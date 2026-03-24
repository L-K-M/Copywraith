<script lang="ts">
	import type { EntryResponse } from './types';

	let {
		entry,
		onstar,
		onview,
		ondownload,
		ondelete
	}: {
		entry: EntryResponse;
		onstar: (id: string, starred: boolean) => void;
		onview: (id: string) => void;
		ondownload: (id: string) => void;
		ondelete: (id: string) => void;
	} = $props();

	function formatDate(iso: string | null): string {
		if (!iso) return '--';
		const d = new Date(iso);
		const pad = (n: number) => String(n).padStart(2, '0');
		return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
	}

	function getPreview(entry: EntryResponse): string {
		if (entry.text_content) {
			return entry.text_content.length > 200
				? entry.text_content.substring(0, 200) + '...'
				: entry.text_content;
		}
		if (entry.content_type === 'file') return '[File]';
		if (entry.content_type === 'image') return '[Image]';
		return '[Empty]';
	}

	function handleStarClick(e: MouseEvent) {
		e.stopPropagation();
		onstar(entry.id, entry.starred);
	}
</script>

<tr>
	<td class="col-star">
		<button
			class="star-btn"
			class:starred={entry.starred}
			title={entry.starred ? 'Unstar' : 'Star'}
			onclick={handleStarClick}
		>
			{entry.starred ? '\u2605' : '\u2606'}
		</button>
	</td>
	<td class="col-type">
		<span class="type-badge">{entry.content_type.toUpperCase()}</span>
	</td>
	<td class="col-content">
		<!-- svelte-ignore a11y_click_events_have_key_events -->
		<!-- svelte-ignore a11y_no_static_element_interactions -->
		<div class="preview" onclick={() => onview(entry.id)}>
			{#if entry.content_type === 'image' && entry.blob_url}
				<img class="img-preview" src={entry.blob_url} alt="clipboard image" />
			{:else}
				{getPreview(entry)}
			{/if}
		</div>
	</td>
	<td class="col-date">{formatDate(entry.created_at)}</td>
	<td class="col-actions">
		<button class="action-btn" onclick={() => onview(entry.id)} title="View">View</button>
		<button class="action-btn" onclick={() => ondownload(entry.id)} title="Download">{'\u2913'}</button>
		<button class="action-btn" onclick={() => ondelete(entry.id)} title="Delete">Del</button>
	</td>
</tr>

<style>
	tr:hover td {
		background: #f0f0f0;
	}

	td {
		vertical-align: top;
	}

	.col-star {
		width: 30px;
		text-align: center;
	}

	.star-btn {
		border: none;
		background: transparent;
		font-size: 14px;
		cursor: pointer;
		padding: 0 2px;
		color: inherit;
	}

	.star-btn.starred {
		color: #f5a623;
	}

	.col-type {
		width: 60px;
	}

	.type-badge {
		display: inline-block;
		padding: 1px 5px;
		border: 1px solid currentColor;
		font-size: 10px;
		text-transform: uppercase;
		opacity: 0.8;
	}

	.col-content {
		max-width: 0;
	}

	.preview {
		max-width: 400px;
		max-height: 2.8em;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-family: "Monaco", "Courier New", monospace;
		font-size: 11px;
		cursor: pointer;
	}

	.img-preview {
		max-width: 120px;
		max-height: 60px;
		border: 1px solid #ccc;
		image-rendering: auto;
	}

	.col-date {
		width: 130px;
		white-space: nowrap;
		font-size: 11px;
	}

	.col-actions {
		width: 110px;
		white-space: nowrap;
	}

	.action-btn {
		font-size: 10px;
		padding: 1px 6px;
		border: 1px solid #999;
		background: #fff;
		cursor: pointer;
		margin-right: 2px;
	}

	.action-btn:hover {
		background: #eee;
	}

	.action-btn:active {
		background: #ddd;
	}
</style>
