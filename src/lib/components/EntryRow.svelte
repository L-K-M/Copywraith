<script lang="ts">
	import type { ClipboardEntry } from '$lib/types';
	import { toggleStar, pasteEntry, pasteEntryPlaintext, deleteEntry } from '$lib/util/clipboardStore';
	import { isMobile } from '$lib/util/platform';
	import { BalloonHelp } from '@lkmc/system7-ui';
	import { TauriService } from '$lib/tauri';
	import { onMount } from 'svelte';

	function formatTime(dateStr: string): string {
		const date = new Date(dateStr);
		const now = new Date();
		const diffMs = now.getTime() - date.getTime();
		const diffSec = Math.floor(diffMs / 1000);
		const diffMin = Math.floor(diffSec / 60);
		const diffHour = Math.floor(diffMin / 60);
		const diffDay = Math.floor(diffHour / 24);

		if (diffSec < 60) return 'now';
		if (diffMin < 60) return `${diffMin}m`;
		if (diffHour < 24) return `${diffHour}h`;
		if (diffDay < 30) return `${diffDay}d`;
		return date.toLocaleDateString();
	}

	function getTypeLabel(type: string): string {
		switch (type) {
			case 'text': return 'TXT';
			case 'html': return 'HTML';
			case 'rtf': return 'RTF';
			case 'image': return 'IMG';
			case 'file': return 'FILE';
			default: return type.toUpperCase();
		}
	}

	let {
		entry,
		isFirst = false,
		selected = false,
		onselect,
		onpreview
	}: {
		entry: ClipboardEntry;
		isFirst?: boolean;
		selected?: boolean;
		onselect?: (id: string) => void;
		onpreview?: (entry: ClipboardEntry) => void;
	} = $props();

	let rowElement: HTMLTableRowElement | null = null;
	let imageData: string | null = $state(null);
	let imageLoading = $state(false);

	// Lazy-load image data only for image entries
	onMount(() => {
		if (entry.has_image) {
			imageLoading = true;
			TauriService.getEntryImage(entry.id).then((data) => {
				imageData = data;
				imageLoading = false;
			}).catch(() => {
				imageLoading = false;
			});
		}
	});

	$effect(() => {
		if (selected && rowElement) {
			rowElement.scrollIntoView({ block: isFirst ? 'start' : 'nearest' });
		}
	});

	function handleClick(e: MouseEvent) {
		onselect?.(entry.id);
		// On mobile, alt-click is not available; always do a standard paste/copy
		if (!$isMobile && e.altKey) {
			pasteEntryPlaintext(entry.id);
		} else {
			pasteEntry(entry.id);
		}
	}

	function handleDblClick(e: MouseEvent) {
		e.preventDefault();
		e.stopPropagation();
		onpreview?.(entry);
	}

	function handleKeydown(e: KeyboardEvent) {
		if (e.key === 'Enter') {
			onselect?.(entry.id);
			pasteEntry(entry.id);
		}
		// Space shows preview
		if (e.key === ' ') {
			e.preventDefault();
			onselect?.(entry.id);
			onpreview?.(entry);
		}
	}

	function handleFocus() {
		onselect?.(entry.id);
	}

	function handleStarClick(e: MouseEvent) {
		e.preventDefault();
		e.stopPropagation();
		toggleStar(entry.id);
	}

	function handleDeleteClick(e: MouseEvent) {
		e.preventDefault();
		e.stopPropagation();
		deleteEntry(entry.id);
	}

	function stopRowClick(e: MouseEvent) {
		e.preventDefault();
		e.stopPropagation();
	}
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<tr
	class="entry-row"
	class:selected={selected}
	bind:this={rowElement}
	onclick={handleClick}
	ondblclick={handleDblClick}
	onfocus={handleFocus}
	onkeydown={handleKeydown}
	tabindex="0"
	role="button"
>
	<td class="col-star">
		<button
			type="button"
			class="star-btn"
			class:starred={entry.starred}
			onmousedown={stopRowClick}
			onclick={handleStarClick}
			title={entry.starred ? 'Unstar' : 'Star'}
		>
			{entry.starred ? '\u2605' : '\u2606'}
		</button>
	</td>
	<td class="col-content">
		{#if entry.has_image && imageData}
			<div class="image-preview">
				<img src="data:image/png;base64,{imageData}" alt="Copied screenshot" />
			</div>
		{:else if entry.has_image && imageLoading}
			<div class="text-preview">[Loading image...]</div>
		{:else if entry.has_image}
			<div class="text-preview">[Image]</div>
		{:else}
			<div class="text-preview" class:sensitive-content={entry.sensitive}>
				{entry.preview}
			</div>
		{/if}
	</td>
	<td class="col-type">
		<span class="type-badge">{getTypeLabel(entry.content_type)}</span>
	</td>
	<td class="col-time">
		<BalloonHelp message={new Date(entry.updated_at).toLocaleString()} delay={600}>
			<span class="time">{formatTime(entry.updated_at)}</span>
		</BalloonHelp>
	</td>
	<td class="col-actions">
		<button
			type="button"
			class="delete-btn"
			onmousedown={stopRowClick}
			onclick={handleDeleteClick}
			title="Delete"
		>
			\u2715
		</button>
	</td>
</tr>

<style>
	.entry-row {
		cursor: pointer;
		user-select: none;
	}

	.entry-row:hover {
		background: var(--system7-color-highlight, #000);
		color: var(--system7-color-highlight-text, #fff);
	}

	.entry-row.selected {
		background: var(--system7-color-highlight, #000);
		color: var(--system7-color-highlight-text, #fff);
	}

	.entry-row:focus {
		background: var(--system7-color-highlight, #000);
		color: var(--system7-color-highlight-text, #fff);
		outline: none;
	}

	.col-star {
		width: 24px;
		text-align: center;
		padding: 2px 4px;
	}

	.star-btn {
		background: none;
		border: none;
		cursor: pointer;
		font-size: 14px;
		padding: 0;
		line-height: 1;
		color: inherit;
	}

	.star-btn.starred {
		color: #f5a623;
	}

	.entry-row:hover .star-btn.starred {
		color: #ffd700;
	}

	.col-content {
		max-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		padding: 3px 6px;
	}

	.text-preview {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-size: 14px;
	}

	.sensitive-content {
		font-style: italic;
		opacity: 0.6;
	}

	.image-preview {
		height: 48px;
		display: flex;
		align-items: center;
	}

	.image-preview img {
		max-height: 48px;
		max-width: 180px;
		image-rendering: auto;
	}

	.col-type {
		width: 40px;
		text-align: center;
		padding: 2px 4px;
	}

	.type-badge {
		font-size: 9px;
		padding: 1px 3px;
		border: 1px solid currentColor;
		border-radius: 2px;
		opacity: 0.7;
	}

	.col-time {
		width: 36px;
		text-align: right;
		padding: 2px 4px;
		font-size: 11px;
		opacity: 0.7;
	}

	.col-actions {
		width: 20px;
		text-align: center;
		padding: 2px 2px;
	}

	.delete-btn {
		background: none;
		border: none;
		cursor: pointer;
		font-size: 10px;
		padding: 0;
		color: inherit;
		opacity: 0;
		transition: opacity 0.1s;
	}

	.entry-row:hover .delete-btn {
		opacity: 0.5;
	}

	.delete-btn:hover {
		opacity: 1 !important;
	}

	/* Mobile: larger touch targets, always-visible actions */
	@media (pointer: coarse) {
		.entry-row {
			min-height: 44px;
		}

		.col-star {
			width: 36px;
			padding: 4px 6px;
		}

		.star-btn {
			font-size: 18px;
			padding: 4px;
		}

		.col-content {
			padding: 6px 8px;
		}

		.text-preview {
			font-size: 15px;
		}

		.image-preview {
			height: 56px;
		}

		.image-preview img {
			max-height: 56px;
		}

		.col-actions {
			width: 32px;
			padding: 4px;
		}

		.delete-btn {
			font-size: 14px;
			opacity: 0.4;
			padding: 4px;
		}
	}
</style>
