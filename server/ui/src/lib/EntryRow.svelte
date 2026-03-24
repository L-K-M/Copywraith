<script lang="ts">
	import {
		BalloonHelp,
		CodeFileIcon,
		DocumentFileIcon,
		DownloadIcon,
		EditIcon,
		GenericFileIcon,
		ImageFileIcon,
		TextFileIcon,
		TrashIcon
	} from '@lkmc/system7-ui';
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
			const text =
				entry.content_type === 'html' ? htmlToPlainText(entry.text_content) : entry.text_content;
			return text.length > 200 ? text.substring(0, 200) + '...' : text;
		}
		if (entry.content_type === 'file') return '[File]';
		if (entry.content_type === 'image') return '[Image]';
		return '[Empty]';
	}

	function htmlToPlainText(html: string): string {
		return html
			.replace(/<style[\s\S]*?<\/style>/gi, ' ')
			.replace(/<script[\s\S]*?<\/script>/gi, ' ')
			.replace(/<[^>]+>/g, ' ')
			.replace(/&nbsp;/gi, ' ')
			.replace(/&amp;/gi, '&')
			.replace(/&lt;/gi, '<')
			.replace(/&gt;/gi, '>')
			.replace(/&quot;/gi, '"')
			.replace(/&#39;/gi, "'")
			.replace(/\s+/g, ' ')
			.trim();
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
		<div class="type-icon-wrap" title={entry.content_type.toUpperCase()}>
			{#if entry.content_type === 'text'}
				<TextFileIcon size={20} alt="Text" />
			{:else if entry.content_type === 'html'}
				<CodeFileIcon size={20} alt="HTML" />
			{:else if entry.content_type === 'rtf'}
				<DocumentFileIcon size={20} alt="RTF" />
			{:else if entry.content_type === 'image'}
				<ImageFileIcon size={20} alt="Image" />
			{:else}
				<GenericFileIcon size={20} alt="File" />
			{/if}
		</div>
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
		<div class="action-group">
			<BalloonHelp message="View entry" delay={350} position="bottom">
				<button
					type="button"
					class="action-btn"
					onclick={() => onview(entry.id)}
					title="View"
					aria-label="View"
				>
					<EditIcon size={16} alt="" />
				</button>
			</BalloonHelp>
			<BalloonHelp message="Download entry" delay={350} position="bottom">
				<button
					type="button"
					class="action-btn"
					onclick={() => ondownload(entry.id)}
					title="Download"
					aria-label="Download"
				>
					<DownloadIcon size={16} alt="" />
				</button>
			</BalloonHelp>
			<BalloonHelp message="Delete entry" delay={350} position="bottom">
				<button
					type="button"
					class="action-btn"
					onclick={() => ondelete(entry.id)}
					title="Delete"
					aria-label="Delete"
				>
					<TrashIcon size={16} alt="" />
				</button>
			</BalloonHelp>
		</div>
	</td>
</tr>

<style>
	tr:hover td {
		background: #f0f0f0;
	}

	tr:hover .col-actions {
		z-index: 5;
	}

	td {
		vertical-align: middle;
	}

	.col-star {
		width: 36px;
		text-align: center;
	}

	.star-btn {
		border: none;
		background: transparent;
		font-size: 20px;
		cursor: pointer;
		padding: 2px 3px;
		color: inherit;
	}

	.star-btn.starred {
		color: #f5a623;
	}

	.col-type {
		width: 56px;
		text-align: left;
		padding: 3px 4px 3px 8px !important;
	}

	.type-icon-wrap {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		width: 20px;
		height: 20px;
		line-height: 0;
		margin: 0;
		position: relative;
		top: 5px;
	}

	.col-content {
		max-width: 0;
	}

	.preview {
		max-width: 680px;
		max-height: 3.2em;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-family: inherit;
		font-size: inherit;
		line-height: 1.4;
		cursor: pointer;
	}

	.img-preview {
		max-width: 150px;
		max-height: 80px;
		border: 1px solid #ccc;
		image-rendering: auto;
	}

	.col-date {
		width: 190px;
		white-space: nowrap;
		font-size: inherit;
	}

	.col-actions {
		width: 132px;
		white-space: nowrap;
		overflow: visible !important;
		position: relative;
		z-index: 2;
	}

	.action-group {
		display: flex;
		align-items: center;
		gap: 6px;
		overflow: visible;
	}

	.action-group :global(.balloon-container) {
		position: relative;
		overflow: visible;
		z-index: 20;
	}

	.action-btn {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		width: 30px;
		height: 28px;
		padding: 0;
		border: none;
		background: transparent;
		cursor: pointer;
	}

	.action-btn :global(.sys7-icon) {
		image-rendering: pixelated;
	}

	.action-btn:hover {
		background: rgba(0, 0, 0, 0.08);
	}

	.action-btn:active {
		background: rgba(0, 0, 0, 0.16);
	}
</style>
