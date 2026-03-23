<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import { getCurrentWindow } from '@tauri-apps/api/window';
	import { listen, type UnlistenFn } from '@tauri-apps/api/event';
	import { TitleBar, Notification, ErrorBanner } from '@lkmc/system7-ui';
	import { WindowManager } from '$lib/windowManager';
	import { windowFocused } from '$lib/util/windowState';
	import { notifications } from '$lib/util/notifications';
	import type { ClipboardEntry } from '$lib/types';
	import {
		loadEntries,
		starredOnly
	} from '$lib/util/clipboardStore';

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

	// Unlisten functions for cleanup
	let unlistenFocus: UnlistenFn;
	let unlistenClipboardUpdated: UnlistenFn;
	let unlistenClipboardReordered: UnlistenFn;
	let unlistenPopupShow: UnlistenFn;

	onMount(async () => {
		// Load initial entries
		await loadEntries();

		// Track window focus
		unlistenFocus = await appWindow.onFocusChanged(({ payload: focused }) => {
			windowFocused.set(focused);
		});

		// Listen for clipboard changes from the Rust backend
		unlistenClipboardUpdated = await listen('clipboard-updated', () => {
			loadEntries();
		});

		unlistenClipboardReordered = await listen('clipboard-reordered', () => {
			loadEntries();
		});

		// Listen for popup show event to set starred filter mode
		unlistenPopupShow = await listen<boolean>('popup-show', (event) => {
			starredOnly.set(event.payload);
			loadEntries();
			// Auto-focus the filter field
			setTimeout(() => {
				filterBar?.focus();
			}, 50);
		});

		// Clipboard monitoring is started from the Rust backend via
		// Clipboard::start_monitor(), so no need to call startListening() here.
	});

	onDestroy(() => {
		unlistenFocus?.();
		unlistenClipboardUpdated?.();
		unlistenClipboardReordered?.();
		unlistenPopupShow?.();
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
		// Escape hides the popup
		if (e.key === 'Escape') {
			windowManager.close();
		}
		// Cmd+, opens settings
		if (e.key === ',' && (e.metaKey || e.ctrlKey)) {
			e.preventDefault();
			showSettings = true;
		}
	}
</script>

<svelte:window onkeydown={handleGlobalKeydown} />

<div class="window-frame s7-root" class:window-unfocused={!$windowFocused}>
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

	{#if !isWindowShaded}
		<Notification notifications={$notifications} />

		{#if errorMessage}
			<ErrorBanner message={errorMessage} onclose={() => (errorMessage = '')} />
		{/if}

		<main class="app-content">
			<FilterBar bind:this={filterBar} />
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
</style>
