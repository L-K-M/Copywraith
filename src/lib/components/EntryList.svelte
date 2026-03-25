<script lang="ts">
	import type { ClipboardEntry } from '$lib/types';
	import { entries, isLoading, selectedEntryId, selectEntry } from '$lib/util/clipboardStore';
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
</script>

<div class="entry-list">
	<DataTable
		{columns}
		loading={$isLoading}
		loadingText="Loading clipboard..."
		empty={$entries.length === 0 && !$isLoading}
		emptyText="No clipboard entries"
	>
		{#each $entries as entry (entry.id)}
			<EntryRow
				{entry}
				selected={$selectedEntryId === entry.id}
				onselect={selectEntry}
				{onpreview}
			/>
		{/each}
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
</style>
