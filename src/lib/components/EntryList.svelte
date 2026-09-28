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

	import { QUICK_PASTE_SLOTS } from '$lib/util/keyboard';
	import { isMobile } from '$lib/util/platform';

	let {
		onpreview,
		quickKeysVisible = false
	}: { onpreview?: (entry: ClipboardEntry) => void; quickKeysVisible?: boolean } = $props();

	/*
	 * These widths are the single source of truth: DataTable renders them as a
	 * colgroup, which wins over any width set on the cells themselves. EntryRow
	 * used to declare a second, contradictory set (star 24px, type 40px, time
	 * 36px, actions 20px) that had no effect but read as if it did.
	 *
	 * The actions column now holds two buttons (preview and delete), so it is
	 * wider than before.
	 */
	const desktopColumns = [
		{ key: 'star', label: '', width: '32px', className: 'col-star-header' },
		{ key: 'content', label: 'Content' },
		{ key: 'type', label: 'Type', width: '78px' },
		{ key: 'time', label: 'Time', width: '72px' },
		{ key: 'actions', label: '', width: '58px' }
	];

	/*
	 * A phone is about 360px wide, so the desktop widths left the copied text
	 * about 100px. Each fixed column here is sized to its content at the
	 * mobile type sizes (16px badge and time, two 32px buttons) plus a
	 * 40px star target; everything else goes to the content column.
	 */
	const mobileColumns = [
		{ key: 'star', label: '', width: '40px', className: 'col-star-header' },
		{ key: 'content', label: 'Content' },
		{ key: 'type', label: 'Type', width: '40px', align: 'center' as const },
		{ key: 'time', label: 'Time', width: '44px', align: 'right' as const },
		{ key: 'actions', label: '', width: '64px' }
	];

	let columns = $derived($isMobile ? mobileColumns : desktopColumns);

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

<div class="entry-list" class:mobile={$isMobile} bind:this={entryListElement}>
	<!--
		On mobile the placeholder only covers the first load: the list reloads
		on every resume and sync, and swapping the rows for "Loading..." each
		time flashed the list and threw the user back to the top. The desktop
		popup keeps it for every load, so stale rows are never clickable while
		a new filter is loading.
	-->
	<DataTable
		{columns}
		bodyClass="entry-list-scroll-body"
		loading={$isLoading && (!$isMobile || $entries.length === 0)}
		loadingText="Loading clipboard..."
		empty={$entries.length === 0 && !$isLoading}
		emptyText="No clipboard entries"
	>
		{#each $entries as entry, index (entry.id)}
			<EntryRow
				{entry}
				isFirst={index === 0}
				selected={$selectedEntryId === entry.id}
				quickKey={quickKeysVisible && index < QUICK_PASTE_SLOTS ? index + 1 : null}
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

	/*
	 * Mobile header labels use Geneva's 1x size, like a Finder list header.
	 * The library's cell rule is (0,3,2), so these selectors need four classes.
	 */
	.entry-list.mobile :global(.s7-data-table-header-container th) {
		padding: 4px 4px;
	}

	.entry-list.mobile :global(.s7-data-table-header-text) {
		font-size: 16px;
	}

	/*
	 * Touch scrolling does not need a 16px grab bar, and it cost 4% of the
	 * width. A thin System 7 thumb still shows where the list is.
	 */
	.entry-list.mobile :global(.entry-list-scroll-body::-webkit-scrollbar) {
		width: 6px;
	}

	.entry-list.mobile :global(.entry-list-scroll-body::-webkit-scrollbar-track) {
		background: var(--system7-color-paper, #fff);
		border-left: 1px solid var(--system7-color-ink, #000);
	}

	.entry-list.mobile :global(.entry-list-scroll-body::-webkit-scrollbar-thumb) {
		background: var(--system7-color-scrollbar-thumb, #ccccff);
		background-image: none;
		border: 1px solid var(--system7-color-ink, #000);
		box-shadow: none;
	}

	.entry-list.mobile :global(.load-more-row td) {
		font-size: 16px;
	}

	.entry-list :global(.load-more-row td) {
		padding: 8px;
		text-align: center;
		font-size: 12px;
		font-style: italic;
		color: #666;
	}
</style>
