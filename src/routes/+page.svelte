<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import { getCurrentWindow } from '@tauri-apps/api/window';
	import { listen, type UnlistenFn } from '@tauri-apps/api/event';
	import { TitleBar, Notification, ErrorBanner } from '@lkmc/system7-ui';
	import { WindowManager } from '$lib/windowManager';
	import { windowFocused } from '$lib/util/windowState';
	import { notifications } from '$lib/util/notifications';
	import { platform, isMobile } from '$lib/util/platform';
	import {
		setSyncEndpointStatus,
		type SyncEndpointStatusInput
	} from '$lib/util/syncStatusStore';
	import type { ClipboardEntry } from '$lib/types';
	import {
		loadEntries,
		moveSelection,
		pasteSelectedEntry,
		selectFirstEntry,
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

	let isWindowShaded = $state(false);
	let showSettings = $state(false);
	let errorMessage = $state('');
	let filterBar: FilterBar | undefined = $state();
	let previewEntry: ClipboardEntry | null = $state(null);

	const AUTO_HIDE_DELAY_MS = 500;
	let autoHideTimer: ReturnType<typeof setTimeout> | null = null;

	// Unlisten functions for cleanup
	let unlistenFocus: UnlistenFn;
	let unlistenClipboardUpdated: UnlistenFn;
	let unlistenClipboardReordered: UnlistenFn;
	let unlistenPopupShow: UnlistenFn;
	let unlistenSyncEndpointStatus: UnlistenFn;
	let unlistenPasteFailed: UnlistenFn;
	let mobileRefreshInFlight = false;

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
		unlistenFocus?.();
		unlistenClipboardUpdated?.();
		unlistenClipboardReordered?.();
		unlistenPopupShow?.();
		unlistenSyncEndpointStatus?.();
		unlistenPasteFailed?.();
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
		setSyncEndpointStatus({
			state: 'checking',
			role: configuredEndpoint.role,
			url: configuredEndpoint.url,
			message: `${reason} Refreshing clipboard and contacting the sync server.`
		});

		const capturePromise = withTimeout(
			TauriService.captureClipboard(),
			5000,
			'Clipboard capture did not respond within 5 seconds.'
		);

		try {
			const result = await withTimeout(
				TauriService.syncNow(),
				45000,
				'Sync did not finish within 45 seconds. Check the server URL and network.'
			);
			setSyncEndpointStatus(result.endpoint_status);
		} catch (e) {
			console.error('Failed to sync entries:', e);
			setSyncEndpointStatus({
				state: 'unreachable',
				role: configuredEndpoint.role,
				url: configuredEndpoint.url,
				message: String(e)
			});
		}

		try {
			await capturePromise;
		} catch (e) {
			console.error('Failed to capture clipboard:', e);
		} finally {
			await loadEntries();
			mobileRefreshInFlight = false;
		}
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
			<EntryList onpreview={(entry) => { previewEntry = entry; }} />
			<StatusBar />
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
	<EntryPreview entry={previewEntry} onclose={() => { previewEntry = null; }} />
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
