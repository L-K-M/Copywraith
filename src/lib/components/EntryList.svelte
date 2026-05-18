<script lang="ts">
	import { onMount, tick } from 'svelte';
	import type { ClipboardEntry } from '$lib/types';
	import {
		entries,
		isLoading,
		isLoadingMore,
		loadMoreEntries,
		selectedEntryId,
		selectEntry
	} from '$lib/util/clipboardStore';
	import { DataTable } from '@lkmc/system7-ui';
	import EntryRow from './EntryRow.svelte';

	let { onpreview }: { onpreview?: (entry: ClipboardEntry) => void } = $props();

	const columns = [
		{ key: 'star', label: '', width: '32px', className: 'col-star-header' },
		{ key: 'content', label: 'Content' },
		{ key: 'type', label: 'Type', width: '78px' },
		{ key: 'time', label: 'Time', width: '72px' },
		{ key: 'actions', label: '', width: '36px' }
	];

	const BOTTOM_LOAD_THRESHOLD_PX = 48;
	let entryListElement: HTMLDivElement | null = null;
	let scrollElement: HTMLElement | null = null;

	onMount(() => {
		let observer: MutationObserver | undefined;

		void tick().then(attachScrollListener);

		if (entryListElement) {
			observer = new MutationObserver(attachScrollListener);
			observer.observe(entryListElement, { childList: true, subtree: true });
		}

		return () => {
			observer?.disconnect();
			scrollElement?.removeEventListener('scroll', handleScroll);
		};
	});

	function attachScrollListener() {
		const nextScrollElement =
			entryListElement?.querySelector<HTMLElement>('.entry-list-scroll-body') ?? null;

		if (nextScrollElement === scrollElement) {
			return;
		}

		scrollElement?.removeEventListener('scroll', handleScroll);
		scrollElement = nextScrollElement;
		scrollElement?.addEventListener('scroll', handleScroll, { passive: true });
	}

	function handleScroll(e: Event) {
		const target = e.target;
		if (!(target instanceof HTMLElement)) {
			return;
		}

		const distanceFromBottom = target.scrollHeight - target.scrollTop - target.clientHeight;
		if (distanceFromBottom <= BOTTOM_LOAD_THRESHOLD_PX) {
			void loadMoreEntries();
		}
	}
</script>

<div class="entry-list" bind:this={entryListElement}>
	<DataTable
		{columns}
		bodyClass="entry-list-scroll-body"
		loading={$isLoading}
		loadingText="Loading clipboard..."
		empty={$entries.length === 0 && !$isLoading}
		emptyText="No clipboard entries"
	>
		{#each $entries as entry, index (entry.id)}
			<EntryRow
				{entry}
				isFirst={index === 0}
				selected={$selectedEntryId === entry.id}
				onselect={selectEntry}
				{onpreview}
			/>
		{/each}

		{#if $isLoadingMore}
			<tr class="load-more-row">
				<td colspan={columns.length}>Loading more...</td>
			</tr>
		{/if}
	</DataTable>
</div>

<style>
	.entry-list {
		flex: 1;
		overflow: hidden;
		display: flex;
		flex-direction: column;
	}

	.entry-list :global(th.col-star-header) {
		font-size: 12px;
		text-align: center;
	}

	.entry-list :global(.entry-list-scroll-body) {
		scrollbar-gutter: stable;
	}

	.entry-list :global(.load-more-row td) {
		padding: 8px;
		text-align: center;
		font-size: 12px;
		font-style: italic;
		color: #666;
	}
</style>
