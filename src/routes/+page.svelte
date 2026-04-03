<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import { getCurrentWindow } from '@tauri-apps/api/window';
	import { listen, type UnlistenFn } from '@tauri-apps/api/event';
	import { TitleBar, Notification, ErrorBanner } from '@lkmc/system7-ui';
	import { WindowManager } from '$lib/windowManager';
	import { windowFocused } from '$lib/util/windowState';
	import { notifications } from '$lib/util/notifications';
	import { platform, isMobile } from '$lib/util/platform';
	import { syncEndpointStatus, type SyncEndpointStatus } from '$lib/util/syncStatusStore';
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

	onMount(async () => {
		// Detect platform first so all components can adapt
		try {
			const p = await TauriService.getPlatform();
			platform.set(p);
		} catch {
			platform.set('');
		}

		// Load initial entries without blocking listener setup
		void loadEntries();

		const mobile = $isMobile;

		// On mobile, capture the current clipboard content on app open
		if (mobile) {
			try {
				await TauriService.captureClipboard();
			} catch (e) {
				console.error('Failed to capture clipboard:', e);
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

			// On mobile, capture clipboard when app resumes (gains focus)
			if (mobile && focused) {
				TauriService.captureClipboard()
					.then((captured: boolean) => {
						if (captured) loadEntries();
					})
					.catch(() => {});
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

		unlistenSyncEndpointStatus = await listen<SyncEndpointStatus>(
			'sync-endpoint-status',
			(event) => {
				const payload = event.payload;
				const state =
					payload.state === 'online' ||
					payload.state === 'disabled' ||
					payload.state === 'unreachable'
						? payload.state
						: 'unreachable';

				syncEndpointStatus.set({
					state,
					role: payload.role ?? null,
					url: payload.url ?? null
				});
			}
		);

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

	function handleSettingsOpen() {
		showSettings = true;
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
</div>

{#if showSettings}
	<SettingsDialog onclose={() => (showSettings = false)} />
{/if}

{#if previewEntry}
	<EntryPreview entry={previewEntry} onclose={() => { previewEntry = null; }} />
{/if}

<style>
	.window-frame {
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
</style>
