<script lang="ts">
	import { onMount } from 'svelte';
	import { listen } from '@tauri-apps/api/event';
	import { TauriService } from '$lib/tauri';
	import { now } from '$lib/util/clock';
	import { notify } from '$lib/util/notifications';
	import {
		NOT_PAUSED,
		PAUSE_CHOICES,
		isPausedAt,
		pauseLabel,
		type CapturePauseStatus
	} from '$lib/util/capturePause';

	let status: CapturePauseStatus = $state(NOT_PAUSED);
	let menuOpen = $state(false);
	let paused = $derived(isPausedAt(status, $now));
	let label = $derived(pauseLabel(status, $now));

	onMount(() => {
		let unlisten: (() => void) | undefined;
		let disposed = false;

		TauriService.getCapturePause()
			.then((current) => {
				if (!disposed) status = current;
			})
			.catch((e) => console.error('Failed to read the capture pause:', e));

		void listen<CapturePauseStatus>('capture-pause-changed', (event) => {
			status = event.payload;
		})
			.then((stop) => {
				if (disposed) stop();
				else unlisten = stop;
			})
			.catch((e) => console.error('Failed to watch the capture pause:', e));

		return () => {
			disposed = true;
			unlisten?.();
		};
	});

	async function choose(minutes: number | null | 'resume') {
		menuOpen = false;
		try {
			status =
				minutes === 'resume'
					? await TauriService.resumeCapture()
					: await TauriService.pauseCapture(minutes);
		} catch (e) {
			notify('error', `Could not change capture: ${e}`);
		}
	}

	function handleMenuKeydown(e: KeyboardEvent) {
		// Close only the menu; Escape elsewhere hides the popup. Captured on the
		// window, before the popup's handler, because WebKit does not focus a
		// clicked button, so focus may be anywhere while the menu is open.
		if (menuOpen && e.key === 'Escape') {
			e.preventDefault();
			e.stopPropagation();
			menuOpen = false;
		}
	}
</script>

<svelte:window
	onclick={() => {
		menuOpen = false;
	}}
	onkeydowncapture={handleMenuKeydown}
/>

<div class="capture-pause">
	<button
		type="button"
		class="pause-button"
		class:paused
		aria-haspopup="menu"
		aria-expanded={menuOpen}
		title={paused
			? 'Copywraith is not recording the clipboard. Click to resume or change.'
			: 'Temporarily stop recording the clipboard'}
		onclick={(e) => {
			e.stopPropagation();
			menuOpen = !menuOpen;
		}}
	>
		{paused ? label : 'Pause'}
	</button>

	{#if menuOpen}
		<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
		<div
			class="pause-menu"
			role="menu"
			tabindex="-1"
			onclick={(e) => e.stopPropagation()}
		>
			{#if paused}
				<button type="button" role="menuitem" onclick={() => choose('resume')}>
					Resume capture
				</button>
				<div class="separator" role="separator"></div>
			{/if}
			{#each PAUSE_CHOICES as choice (choice.label)}
				<button type="button" role="menuitem" onclick={() => choose(choice.minutes)}>
					{choice.label}
				</button>
			{/each}
		</div>
	{/if}
</div>

<style>
	.capture-pause {
		position: relative;
	}

	.pause-button {
		padding: 2px 6px;
		border: 1px solid #777;
		background: #f5f5f5;
		color: inherit;
		font: inherit;
		font-size: 13px;
		white-space: nowrap;
		cursor: pointer;
	}

	/* A sleeping ghost is hard to miss: full inversion, like a selected item. */
	.pause-button.paused {
		border-color: #000;
		background: #000;
		color: #fff;
	}

	.pause-menu {
		position: absolute;
		right: 0;
		bottom: calc(100% + 4px);
		z-index: 30;
		display: flex;
		flex-direction: column;
		min-width: 170px;
		padding: 2px 0;
		border: 1px solid #000;
		background: #fff;
		box-shadow: 2px 2px 0 #000;
	}

	.pause-menu button {
		padding: 3px 14px;
		border: none;
		background: none;
		color: inherit;
		font: inherit;
		font-size: 13px;
		text-align: left;
		white-space: nowrap;
		cursor: pointer;
	}

	.pause-menu button:hover,
	.pause-menu button:focus-visible {
		background: #000;
		color: #fff;
		outline: none;
	}

	.separator {
		margin: 2px 0;
		border-top: 1px dotted #000;
	}
</style>
