<script lang="ts">
	import type { ClipboardEntry } from '$lib/types';
	import { pasteEntry, pasteEntryPlaintext, toggleStar, deleteEntry } from '$lib/util/clipboardStore';
	import { MovableDialog, Button } from '@lkmc/system7-ui';
	import { TauriService } from '$lib/tauri';

	let { entry, onclose }: { entry: ClipboardEntry; onclose: () => void } = $props();

	let imageData: string | null = $state(null);
	// Filled in only when the list projection had to truncate. Until then the
	// dialog renders the prefix the list already carries, so there is no blank
	// frame while the fetch is in flight.
	let fetchedText: string | null = $state(null);
	let isLoadingFullText = $state(false);

	let fullText = $derived(fetchedText ?? entry.full_text);

	$effect(() => {
		const id = entry.id;
		const hasImage = entry.has_image;
		const needsFullText = entry.full_text_truncated;
		let disposed = false;

		imageData = null;
		fetchedText = null;
		isLoadingFullText = false;

		if (hasImage) {
			TauriService.getEntryImage(id)
				.then((data) => {
					if (!disposed) imageData = data;
				})
				.catch(() => {});
		}

		if (needsFullText) {
			isLoadingFullText = true;
			TauriService.getEntryText(id)
				.then((text) => {
					if (!disposed && text) fetchedText = text;
				})
				.catch((e) => {
					// The truncated prefix stays on screen, so this is not fatal.
					console.error('Failed to load full entry text:', e);
				})
				.finally(() => {
					if (!disposed) isLoadingFullText = false;
				});
		}

		return () => {
			disposed = true;
		};
	});

	function formatDateTime(dateStr: string): string {
		const date = new Date(dateStr);
		return date.toLocaleString();
	}

	function getTypeLabel(type: string): string {
		switch (type) {
			case 'text':
				return 'Plain Text';
			case 'html':
				return 'HTML';
			case 'rtf':
				return 'Rich Text';
			case 'image':
				return 'Image';
			case 'file':
				return 'File';
			default:
				return type;
		}
	}

	async function handlePaste() {
		await pasteEntry(entry.id);
		onclose();
	}

	async function handlePastePlaintext() {
		await pasteEntryPlaintext(entry.id);
		onclose();
	}

	function handleStar() {
		toggleStar(entry.id);
	}

	function handleDelete() {
		deleteEntry(entry.id);
		onclose();
	}
</script>

<MovableDialog title="Entry Preview" onclose={onclose} width="420px">
	<div class="preview-content">
		<div class="meta-row">
			<span class="meta-label">Type:</span>
			<span class="meta-value">{getTypeLabel(entry.content_type)}</span>
		</div>
		<div class="meta-row">
			<span class="meta-label">Created:</span>
			<span class="meta-value">{formatDateTime(entry.created_at)}</span>
		</div>
		{#if entry.source_app}
			<div class="meta-row">
				<span class="meta-label">Source:</span>
				<span class="meta-value">{entry.source_app}</span>
			</div>
		{/if}
		<div class="meta-row">
			<span class="meta-label">Starred:</span>
			<span class="meta-value">{entry.starred ? 'Yes' : 'No'}</span>
		</div>
		{#if entry.sensitive}
			<div class="meta-row">
				<span class="meta-label">Sensitive:</span>
				<span class="meta-value sensitive-label">Yes</span>
			</div>
		{/if}

		<div class="content-display">
			{#if entry.has_image && imageData}
				<div class="image-container">
					<img src="data:image/png;base64,{imageData}" alt="Clipboard preview" />
				</div>
			{:else if entry.has_image}
				<div class="empty-content">Loading image...</div>
			{:else if fullText}
				<pre class="text-content" class:sensitive-content={entry.sensitive}>{fullText}</pre>
				{#if isLoadingFullText}
					<div class="loading-more" role="status">Loading the rest of this entry...</div>
				{/if}
			{:else if entry.preview}
				<pre class="text-content" class:sensitive-content={entry.sensitive}>{entry.preview}</pre>
			{:else}
				<div class="empty-content">No displayable content</div>
			{/if}
		</div>

		<div class="actions">
			<Button onclick={handlePaste}>Paste</Button>
			<Button onclick={handlePastePlaintext}>Paste as Text</Button>
			<Button onclick={handleStar}>{entry.starred ? 'Unstar' : 'Star'}</Button>
			<Button onclick={handleDelete}>Delete</Button>
		</div>
	</div>
</MovableDialog>

<style>
	.preview-content {
		padding: 8px;
	}

	.meta-row {
		display: flex;
		gap: 8px;
		font-size: 11px;
		padding: 2px 0;
	}

	.meta-label {
		font-weight: bold;
		width: 60px;
		flex-shrink: 0;
	}

	.meta-value {
		color: #444;
	}

	.content-display {
		margin-top: 8px;
		border: 1px solid #000;
		border-right-color: #fff;
		border-bottom-color: #fff;
		background: #fff;
		min-height: 60px;
		max-height: 240px;
		overflow-y: auto;
	}

	.text-content {
		font-family: 'Monaco', 'Courier New', monospace;
		font-size: 11px;
		padding: 6px;
		margin: 0;
		white-space: pre-wrap;
		word-break: break-all;
	}

	.image-container {
		padding: 4px;
		display: flex;
		align-items: center;
		justify-content: center;
	}

	.image-container img {
		max-width: 100%;
		max-height: 220px;
		image-rendering: auto;
	}

	.empty-content {
		padding: 16px;
		text-align: center;
		color: #888;
		font-size: 11px;
	}

	.loading-more {
		padding: 4px 6px;
		border-top: 1px solid #ddd;
		color: #888;
		font-size: 10px;
		font-style: italic;
	}

	.sensitive-content {
		font-style: italic;
	}

	.sensitive-label {
		color: #c44;
		font-weight: bold;
	}

	.actions {
		display: flex;
		gap: 8px;
		margin-top: 10px;
		justify-content: flex-end;
	}
</style>
