<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import { getCurrentWindow } from '@tauri-apps/api/window';
	import { listen, type UnlistenFn } from '@tauri-apps/api/event';
	import { TitleBar, Notification, ErrorBanner, ModalDialog, ProgressBar } from '@lkmc/system7-ui';
	import { WindowManager } from '$lib/windowManager';
	import { windowFocused } from '$lib/util/windowState';
	import { notifications } from '$lib/util/notifications';
	import { platform, isMobile } from '$lib/util/platform';
	import {
		setSyncEndpointStatus,
		type SyncEndpointStatusInput
	} from '$lib/util/syncStatusStore';
	import {
		loadEntries,
		moveSelection,
		pasteSelectedEntry,
		selectFirstEntry,
		entries,
		starredOnly
	} from '$lib/util/clipboardStore';
	import { notify } from '$lib/util/notifications';
	import { TauriService } from '$lib/tauri';

	import FilterBar from '$lib/components/FilterBar.svelte';
	import EntryList from '$lib/components/EntryList.svelte';
	import EntryPreview from '$lib/components/EntryPreview.svelte';
	import StatusBar from '$lib/components/StatusBar.svelte';
	import SettingsDialog from '$lib/components/SettingsDialog.svelte';

	const appWindow = getCurrentWindow();
	const windowManager = new WindowManager();
	type ProgressTone = 'normal' | 'success' | 'error';

	let isWindowShaded = $state(false);
	let showSettings = $state(false);
	let errorMessage = $state('');
	let filterBar: FilterBar | undefined = $state();
	let previewEntryId: string | null = $state(null);
	let previewEntry = $derived(
		previewEntryId ? $entries.find((entry) => entry.id === previewEntryId) ?? null : null
	);
	let shareProgressVisible = $state(false);
	let shareProgressValue = $state(0);
	let shareProgressTitle = $state('Importing Shared Items');
	let shareProgressMessage = $state('Preparing Android share import...');
	let shareProgressDetail = $state('');
	let mobileSyncProgressVisible = $state(false);
	let mobileSyncProgressValue = $state(0);
	let mobileSyncProgressLabel = $state('Preparing sync...');
	let mobileSyncProgressDetail = $state('');
	let mobileSyncProgressTone: ProgressTone = $state('normal');

	const AUTO_HIDE_DELAY_MS = 500;
	let autoHideTimer: ReturnType<typeof setTimeout> | null = null;

	// Unlisten functions for cleanup
	let unlistenFocus: UnlistenFn;
	let unlistenClipboardUpdated: UnlistenFn;
	let unlistenClipboardReordered: UnlistenFn;
	let unlistenPopupShow: UnlistenFn;
	let unlistenSyncEndpointStatus: UnlistenFn;
	let unlistenPasteFailed: UnlistenFn;
	let unlistenShizukuClipboardStaged: { unregister: () => Promise<void> } | undefined;
	let mobileRefreshInFlight = false;
	let shareProgressHideTimer: ReturnType<typeof setTimeout> | null = null;
	let mobileSyncProgressHideTimer: ReturnType<typeof setTimeout> | null = null;

	onMount(async () => {
		// Detect platform first so all components can adapt
		let detectedPlatform = '';
		try {
			detectedPlatform = await TauriService.getPlatform();
			platform.set(detectedPlatform);
		} catch {
			platform.set('');
		}

		try {
			unlistenSyncEndpointStatus = await listen<SyncEndpointStatusInput>(
				'sync-endpoint-status',
				(event) => setSyncEndpointStatus(event.payload)
			);
		} catch (e) {
			console.error('Failed to listen for sync endpoint status:', e);
		}

		const mobile = detectedPlatform === 'android' || detectedPlatform === 'ios';

		// Load cached entries immediately, then refresh mobile from clipboard/server.
		void loadEntries();

		if (mobile) {
			void refreshMobileEntries('App opened on mobile.');
		}

		if (detectedPlatform === 'android') {
			try {
				unlistenShizukuClipboardStaged = await TauriService.onShizukuClipboardStaged(() => {
					void refreshMobileEntries('Shizuku captured Android clipboard.');
				});
			} catch (e) {
				console.debug('Shizuku clipboard listener events unavailable:', e);
			}
		}

		// Track window focus
		unlistenFocus = await appWindow.onFocusChanged(({ payload: focused }) => {
			windowFocused.set(focused);

			if (focused) {
				if (autoHideTimer) {
					clearTimeout(autoHideTimer);
					autoHideTimer = null;
				}
			} else if (!mobile) {
				if (autoHideTimer) clearTimeout(autoHideTimer);
				autoHideTimer = setTimeout(() => {
					autoHideTimer = null;
					windowManager.close();
				}, AUTO_HIDE_DELAY_MS);
			}

			// On mobile, refresh clipboard and server state when the app resumes.
			if (mobile && focused) {
				void refreshMobileEntries('App resumed on mobile.');
			}
		});

		// Listen for clipboard changes from the Rust backend
		unlistenClipboardUpdated = await listen('clipboard-updated', () => {
			loadEntries();
		});

		unlistenClipboardReordered = await listen('clipboard-reordered', () => {
			loadEntries();
		});

		// Desktop-only: Listen for popup show event to set starred filter mode
		if (!mobile) {
			unlistenPopupShow = await listen<boolean>('popup-show', (event) => {
				starredOnly.set(event.payload);
				selectFirstEntry({ forceReselect: true });
				loadEntries({ forceSelectFirst: true });
				// Auto-focus the filter field
				setTimeout(() => {
					filterBar?.focus();
				}, 50);
			});
		}

		// Desktop: show user-visible feedback when paste simulation fails
		// (e.g. missing Accessibility permission).
		if (!mobile) {
			unlistenPasteFailed = await listen<string>('paste-failed', (event) => {
				notify('error', event.payload, 6000);
			});
		}

		// Clipboard monitoring is started from the Rust backend via
		// Clipboard::start_monitor() on desktop, so no need to call startListening() here.
	});

	onDestroy(() => {
		if (autoHideTimer) {
			clearTimeout(autoHideTimer);
			autoHideTimer = null;
		}
		if (shareProgressHideTimer) {
			clearTimeout(shareProgressHideTimer);
			shareProgressHideTimer = null;
		}
		if (mobileSyncProgressHideTimer) {
			clearTimeout(mobileSyncProgressHideTimer);
			mobileSyncProgressHideTimer = null;
		}
		unlistenFocus?.();
		unlistenClipboardUpdated?.();
		unlistenClipboardReordered?.();
		unlistenPopupShow?.();
		unlistenSyncEndpointStatus?.();
		unlistenPasteFailed?.();
		void unlistenShizukuClipboardStaged?.unregister();
	});

	function handleWindowClose() {
		windowManager.close();
	}

	async function handleWindowShade() {
		isWindowShaded = await windowManager.toggleShade();
	}

	function handleWindowDrag(e: MouseEvent | TouchEvent) {
		windowManager.startDragging();
	}

	function handleWindowResize(direction: Parameters<WindowManager['startResizeDragging']>[0]) {
		void windowManager.startResizeDragging(direction);
	}

	function handleSettingsOpen() {
		showSettings = true;
	}

	async function refreshMobileEntries(reason = 'Manual mobile refresh.') {
		if (mobileRefreshInFlight) return;
		mobileRefreshInFlight = true;
		const configuredEndpoint = await getConfiguredSyncEndpoint();
		let hadWarning = false;
		showMobileSyncProgress('Preparing mobile sync...', 5, reason);
		setSyncEndpointStatus({
			state: 'checking',
			role: configuredEndpoint.role,
			url: configuredEndpoint.url,
			message: `${reason} Refreshing clipboard and contacting the sync server.`
		});


		if ($platform === 'android') {
			try {
				const pendingShares = await withTimeout(
					TauriService.hasPendingShares(),
					5000,
					'Android shared-item check did not respond within 5 seconds.'
				);

				if (pendingShares.pending) {
					showShareProgress(
						'Importing Shared Items',
						pendingShares.staged
							? 'Reading the current Android share-sheet payload.'
							: 'Reading staged Android share-sheet payloads.',
						10
					);

					try {
						updateShareProgress('Importing Shared Items', 'Saving shared items locally.', 45);
						const shareResult = await withTimeout(
							TauriService.importPendingShares(),
							10000,
							'Android shared-item import did not respond within 10 seconds.'
						);
						if (shareResult.imported > 0) {
							updateShareProgress(
								'Updating List',
								`Imported ${shareResult.imported} shared item${shareResult.imported === 1 ? '' : 's'} locally.`,
								80
							);
							await loadEntries();
							notify(
								'success',
								`Imported ${shareResult.imported} shared item${shareResult.imported === 1 ? '' : 's'}.`
							);
							updateShareProgress('Share Import Complete', 'Shared items are now in local history.', 100);
						} else {
							updateShareProgress('No Shared Items Imported', 'No new shared items were waiting.', 100);
						}
					} catch (e) {
						hadWarning = true;
						console.error('Failed to import Android shared items:', e);
						updateShareProgress('Share Import Failed', 'Could not import shared items.', 100, String(e));
					} finally {
						scheduleShareProgressHide();
					}
				}
			} catch (e) {
				hadWarning = true;
				console.error('Failed to check Android shared items:', e);
			}
		}

		try {
			updateMobileSyncProgress('Capturing clipboard...', 30, 'Checking the current mobile clipboard.');
			const captured = await withTimeout(
				TauriService.captureClipboard(),
				5000,
				'Clipboard capture did not respond within 5 seconds.'
			);
			if (captured) {
				updateMobileSyncProgress('Clipboard captured locally.', 45, 'Updating local history before server sync.');
				await loadEntries();
			} else {
				updateMobileSyncProgress('Clipboard capture complete.', 45);
			}
		} catch (e) {
			hadWarning = true;
			console.error('Failed to capture clipboard:', e);
			updateMobileSyncProgress('Clipboard capture failed.', 45, String(e), 'error');
		}

		try {
			updateMobileSyncProgress('Syncing with server...', 65, 'Uploading local entries and pulling server changes.');
			const result = await withTimeout(
				TauriService.syncNow(),
				45000,
				'Sync did not finish within 45 seconds. Check the server URL and network.'
			);
			setSyncEndpointStatus(result.endpoint_status);
			updateMobileSyncProgress(
				result.pulled > 0
					? `Pulled ${result.pulled} server entr${result.pulled === 1 ? 'y' : 'ies'}.`
					: 'Server sync complete.',
				85
			);
		} catch (e) {
			hadWarning = true;
			console.error('Failed to sync entries:', e);
			updateMobileSyncProgress('Server sync failed.', 85, String(e), 'error');
		}

		try {
			updateMobileSyncProgress('Reloading history...', 92, 'Refreshing local clipboard history.');
			await loadEntries();
			updateMobileSyncProgress(
				hadWarning ? 'Refresh finished with warnings.' : 'Clipboard history is up to date.',
				100,
				hadWarning ? 'Open Sync Details if entries are missing.' : '',
				hadWarning ? 'error' : 'success'
			);
		} catch (e) {
			console.error('Failed to reload clipboard history:', e);
			updateMobileSyncProgress('List refresh failed.', 100, String(e), 'error');
		} finally {
			mobileRefreshInFlight = false;
			scheduleMobileSyncProgressHide();
		}
	}

	function showShareProgress(title: string, message: string, value: number, detail = '') {
		if (shareProgressHideTimer) {
			clearTimeout(shareProgressHideTimer);
			shareProgressHideTimer = null;
		}
		shareProgressVisible = true;
		updateShareProgress(title, message, value, detail);
	}

	function updateShareProgress(title: string, message: string, value: number, detail = '') {
		shareProgressTitle = title;
		shareProgressMessage = message;
		shareProgressValue = value;
		shareProgressDetail = detail;
	}

	function scheduleShareProgressHide() {
		if (shareProgressHideTimer) clearTimeout(shareProgressHideTimer);
		shareProgressHideTimer = setTimeout(() => {
			shareProgressVisible = false;
			shareProgressHideTimer = null;
		}, 900);
	}

	function showMobileSyncProgress(label: string, value: number, detail = '', tone: ProgressTone = 'normal') {
		if (mobileSyncProgressHideTimer) {
			clearTimeout(mobileSyncProgressHideTimer);
			mobileSyncProgressHideTimer = null;
		}
		mobileSyncProgressVisible = true;
		updateMobileSyncProgress(label, value, detail, tone);
	}

	function updateMobileSyncProgress(label: string, value: number, detail = '', tone: ProgressTone = 'normal') {
		mobileSyncProgressLabel = label;
		mobileSyncProgressValue = value;
		mobileSyncProgressDetail = detail;
		mobileSyncProgressTone = tone;
	}

	function scheduleMobileSyncProgressHide() {
		if (mobileSyncProgressHideTimer) clearTimeout(mobileSyncProgressHideTimer);
		mobileSyncProgressHideTimer = setTimeout(() => {
			mobileSyncProgressVisible = false;
			mobileSyncProgressHideTimer = null;
		}, 1800);
	}

	async function getConfiguredSyncEndpoint() {
		try {
			const settings = await TauriService.getSettings();
			if (settings.server_url_primary) {
				return { role: 'local', url: settings.server_url_primary };
			}
			if (settings.server_url_fallback) {
				return { role: 'vpn', url: settings.server_url_fallback };
			}
		} catch (e) {
			console.error('Failed to read sync settings:', e);
		}

		return { role: null, url: null };
	}

	function withTimeout<T>(promise: Promise<T>, timeoutMs: number, timeoutMessage: string) {
		let timeoutId: ReturnType<typeof setTimeout> | undefined;
		const timeout = new Promise<T>((_, reject) => {
			timeoutId = setTimeout(() => reject(new Error(timeoutMessage)), timeoutMs);
		});

		return Promise.race([promise.finally(() => clearTimeout(timeoutId)), timeout]);
	}

	function handleGlobalKeydown(e: KeyboardEvent) {
		// Skip keyboard shortcuts on mobile
		if ($isMobile) return;

		const target = e.target;
		const isInputTarget =
			target instanceof HTMLElement &&
			(target.tagName === 'INPUT' ||
				target.tagName === 'TEXTAREA' ||
				target.tagName === 'SELECT' ||
				target.isContentEditable);

		// Escape hides the popup
		if (e.key === 'Escape') {
			e.preventDefault();
			if (previewEntryId) {
				previewEntryId = null;
				return;
			}
			if (showSettings) {
				showSettings = false;
				return;
			}
			windowManager.close();
			return;
		}

		// Cmd+, opens settings
		if (e.key === ',' && (e.metaKey || e.ctrlKey)) {
			e.preventDefault();
			showSettings = true;
			return;
		}

		if (isInputTarget) {
			return;
		}

		if (e.key === 'ArrowDown') {
			e.preventDefault();
			moveSelection(1);
			return;
		}

		if (e.key === 'ArrowUp') {
			e.preventDefault();
			moveSelection(-1);
			return;
		}

		if (e.key === 'Enter') {
			e.preventDefault();
			pasteSelectedEntry();
		}
	}
</script>

<svelte:window onkeydown={handleGlobalKeydown} />

<div
	class="window-frame s7-root"
	class:window-unfocused={!$windowFocused}
	class:mobile={$isMobile}
	class:android={$platform === 'android'}
>
	{#if !$isMobile}
		<TitleBar
			title="Copywraith"
			focused={$windowFocused}
			closable
			shadeable
			draggable
			onclose={handleWindowClose}
			onshade={handleWindowShade}
			ondragstart={handleWindowDrag}
		/>
	{/if}

	{#if $isMobile}
		<div class="mobile-safe-top" aria-hidden="true"></div>
	{/if}

	{#if !isWindowShaded}
		<Notification notifications={$notifications} />

		{#if errorMessage}
			<ErrorBanner message={errorMessage} onclose={() => (errorMessage = '')} />
		{/if}

		<main class="app-content">
			<FilterBar bind:this={filterBar} onsettings={handleSettingsOpen} />
			<EntryList onpreview={(entry) => { previewEntryId = entry.id; }} />
			<StatusBar
				progressVisible={mobileSyncProgressVisible}
				progressValue={mobileSyncProgressValue}
				progressTone={mobileSyncProgressTone}
			/>
		</main>
	{/if}

	{#if !$isMobile && !isWindowShaded}
		<button
			type="button"
			class="resize-handle resize-handle-n"
			onmousedown={() => handleWindowResize('North')}
			aria-label="Resize north"
		></button>
		<button
			type="button"
			class="resize-handle resize-handle-e"
			onmousedown={() => handleWindowResize('East')}
			aria-label="Resize east"
		></button>
		<button
			type="button"
			class="resize-handle resize-handle-s"
			onmousedown={() => handleWindowResize('South')}
			aria-label="Resize south"
		></button>
		<button
			type="button"
			class="resize-handle resize-handle-w"
			onmousedown={() => handleWindowResize('West')}
			aria-label="Resize west"
		></button>
		<button
			type="button"
			class="resize-handle resize-handle-ne"
			onmousedown={() => handleWindowResize('NorthEast')}
			aria-label="Resize north east"
		></button>
		<button
			type="button"
			class="resize-handle resize-handle-se"
			onmousedown={() => handleWindowResize('SouthEast')}
			aria-label="Resize south east"
		></button>
		<button
			type="button"
			class="resize-handle resize-handle-sw"
			onmousedown={() => handleWindowResize('SouthWest')}
			aria-label="Resize south west"
		></button>
		<button
			type="button"
			class="resize-handle resize-handle-nw"
			onmousedown={() => handleWindowResize('NorthWest')}
			aria-label="Resize north west"
		></button>
	{/if}
</div>

{#if showSettings}
	<SettingsDialog onclose={() => (showSettings = false)} />
{/if}

{#if previewEntry}
	<EntryPreview entry={previewEntry} onclose={() => { previewEntryId = null; }} />
{/if}

{#if shareProgressVisible && $isMobile}
	<ModalDialog width="340px">
		<div class="mobile-progress-dialog">
			<div class="progress-title">{shareProgressTitle}</div>
			<ProgressBar
				value={shareProgressValue}
				max={100}
				height={16}
				ariaLabel="Shared item import progress"
			/>
			<div class="progress-message">{shareProgressMessage}</div>
			{#if shareProgressDetail}
				<div class="progress-detail">{shareProgressDetail}</div>
			{/if}
		</div>
	</ModalDialog>
{/if}

<style>
	.window-frame {
		position: relative;
		display: flex;
		flex-direction: column;
		height: 100vh;
		height: 100dvh;
		border: 2px solid #000;
		box-sizing: border-box;
		background: #fff;
		overflow: hidden;
	}

	.window-unfocused {
		border-color: #999;
	}

	.app-content {
		display: flex;
		flex-direction: column;
		flex: 1;
		overflow: hidden;
	}

	/* Mobile: no window border, full screen */
	.window-frame.mobile {
		--safe-area-top: env(safe-area-inset-top, 0px);
		--safe-area-bottom: env(safe-area-inset-bottom, 0px);
		border: none;
	}

	.window-frame.mobile.android {
		--safe-area-top: max(env(safe-area-inset-top, 0px), 24px);
	}

	.mobile-safe-top {
		height: var(--safe-area-top);
		flex-shrink: 0;
	}

	.window-frame.mobile .app-content {
		padding-bottom: var(--safe-area-bottom);
	}

	.mobile-progress-dialog {
		display: flex;
		flex-direction: column;
		gap: 10px;
		padding: 8px 4px;
		font-size: 12px;
	}

	.progress-title {
		font-weight: bold;
		text-align: center;
	}

	.progress-message {
		line-height: 1.35;
		text-align: center;
	}

	.progress-detail {
		max-height: 72px;
		overflow: auto;
		font-size: 10px;
		line-height: 1.3;
		color: #555;
		word-break: break-word;
		border-top: 1px solid #bbb;
		padding-top: 6px;
	}

	.resize-handle {
		position: absolute;
		z-index: 20;
		padding: 0;
		border: 0;
		background: transparent;
	}

	.resize-handle-n,
	.resize-handle-s {
		left: 14px;
		right: 14px;
		height: 6px;
		cursor: ns-resize;
	}

	.resize-handle-e,
	.resize-handle-w {
		top: 39px;
		bottom: 14px;
		width: 6px;
		cursor: ew-resize;
	}

	.resize-handle-n {
		top: -1px;
	}

	.resize-handle-e {
		right: -1px;
	}

	.resize-handle-s {
		bottom: -1px;
	}

	.resize-handle-w {
		left: -1px;
	}

	.resize-handle-ne,
	.resize-handle-se,
	.resize-handle-sw,
	.resize-handle-nw {
		width: 14px;
		height: 14px;
	}

	.resize-handle-ne,
	.resize-handle-sw {
		cursor: nesw-resize;
	}

	.resize-handle-se,
	.resize-handle-nw {
		cursor: nwse-resize;
	}

	.resize-handle-ne {
		top: -1px;
		right: -1px;
	}

	.resize-handle-se {
		right: -1px;
		bottom: -1px;
	}

	.resize-handle-sw {
		bottom: -1px;
		left: -1px;
	}

	.resize-handle-nw {
		top: -1px;
		left: -1px;
	}
</style>
