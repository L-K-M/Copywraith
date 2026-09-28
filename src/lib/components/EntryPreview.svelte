<script lang="ts">
	import type { ClipboardEntry } from '$lib/types';
	import { pasteEntry, pasteEntryPlaintext, toggleStar, deleteEntry } from '$lib/util/clipboardStore';
	import { MovableDialog, Button } from '@lkmc/system7-ui';
	import { TauriService } from '$lib/tauri';
	import { isMobile } from '$lib/util/platform';

	let { entry, onclose }: { entry: ClipboardEntry; onclose: () => void } = $props();

	let imageData: string | null = $state(null);
	// Filled in only when the list projection had to truncate. Until then the
	// dialog renders the prefix the list already carries, so there is no blank
	// frame while the fetch is in flight.
	let fetchedText: string | null = $state(null);
	let isLoadingFullText = $state(false);
	let fullTextFailed = $state(false);
	let imageFailed = $state(false);

	let fullText = $derived(fetchedText ?? entry.full_text);

	$effect(() => {
		const id = entry.id;
		const hasImage = entry.has_image;
		const needsFullText = entry.full_text_truncated;
		let disposed = false;

		imageData = null;
		fetchedText = null;
		isLoadingFullText = false;
		fullTextFailed = false;
		imageFailed = false;

		if (hasImage) {
			TauriService.getEntryImage(id)
				.then((data) => {
					if (disposed) return;
					imageData = data;
					// A null payload means the blob is gone, not that it is
					// still arriving — otherwise the dialog would sit on
					// "Loading image..." forever.
					imageFailed = data === null;
				})
				.catch((e) => {
					console.error('Failed to load entry image:', e);
					if (!disposed) imageFailed = true;
				});
		}

		if (needsFullText) {
			isLoadingFullText = true;
			TauriService.getEntryText(id)
				.then((text) => {
					// `!= null` rather than a truthiness check: an empty string is
					// a successful fetch, not a missing one.
					if (!disposed && text != null) fetchedText = text;
				})
				.catch((e) => {
					// The truncated prefix stays on screen, so this is not fatal
					// — but the user must be told that what they see is partial.
					console.error('Failed to load full entry text:', e);
					if (!disposed) fullTextFailed = true;
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

<!-- On a phone the width is capped by the viewport; the extra width only
	matters in landscape, where the preview switches to two columns. -->
<MovableDialog title="Entry Preview" onclose={onclose} width={$isMobile ? '720px' : '420px'}>
	<div class="preview-content" class:mobile={$isMobile}>
		<div class="meta">
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
		</div>

		<div class="content-display">
			{#if entry.has_image && imageData}
				<div class="image-container">
					<img src="data:image/png;base64,{imageData}" alt="Clipboard preview" />
				</div>
			{:else if entry.has_image && imageFailed}
				<div class="empty-content failed" role="alert">
					This image could not be loaded. The stored file may be missing.
				</div>
			{:else if entry.has_image}
				<div class="empty-content">Loading image...</div>
			{:else if fullText}
				<pre class="text-content" class:sensitive-content={entry.sensitive}>{fullText}</pre>
				{#if isLoadingFullText}
					<div class="loading-more" role="status">Loading the rest of this entry...</div>
				{:else if fullTextFailed}
					<div class="loading-more failed" role="alert">
						Showing a shortened version — the rest of this entry could not be loaded.
					</div>
				{/if}
			{:else if entry.preview}
				<pre class="text-content" class:sensitive-content={entry.sensitive}>{entry.preview}</pre>
			{:else}
				<div class="empty-content">No displayable content</div>
			{/if}
		</div>

		<div class="actions">
			<!-- Android cannot paste into another app; these only copy. -->
			<Button onclick={handlePaste}>{$isMobile ? 'Copy' : 'Paste'}</Button>
			<Button onclick={handlePastePlaintext}>{$isMobile ? 'Copy as Text' : 'Paste as Text'}</Button>
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

	.loading-more.failed {
		color: #a01717;
		font-style: normal;
	}

	.empty-content.failed {
		color: #a01717;
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

	/*
	 * Phone layout. The dialog sits inside .s7-root there, so every element
	 * without its own font-size would be Geneva 24px and the desktop 10-11px
	 * sizes are illegible in Geneva; 16px is the font's crisp 1x size.
	 *
	 * The text box is capped so the whole dialog fits the viewport: 93px is
	 * the backdrop margin plus the dialog frame, title bar and body padding,
	 * 3px covers the box border and rounding (96px in landscape), and portrait
	 * also reserves 236px for five meta rows and two button rows (332px).
	 * The flex column below still shrinks the box if that estimate is off.
	 */
	.preview-content.mobile {
		--preview-box-max: calc(
			100vh - env(safe-area-inset-top, 0px) - env(safe-area-inset-bottom, 0px) - 332px
		);
		display: flex;
		flex-direction: column;
		flex: 1 1 auto;
		min-height: 0;
		padding: 0;
	}

	@supports (height: 100dvh) {
		.preview-content.mobile {
			--preview-box-max: calc(
				100dvh - env(safe-area-inset-top, 0px) - env(safe-area-inset-bottom, 0px) - 332px
			);
		}
	}

	.mobile .meta-row {
		line-height: 20px;
	}

	.mobile .meta-label,
	.mobile .meta-value {
		font-size: 16px;
	}

	.mobile .meta-value {
		min-width: 0;
		overflow-wrap: anywhere;
	}

	.mobile .content-display {
		flex: 0 1 auto;
		min-height: 96px;
		max-height: var(--preview-box-max);
	}

	/* Monospace is kept on purpose: copied text is often code or commands,
	   where indentation and look-alike characters (0/O, l/1) matter. The
	   library forces Geneva with !important, so this has to as well. */
	.mobile .text-content {
		font-family: 'Monaco', 'Courier New', monospace !important;
		font-size: 16px;
		line-height: 1.35;
		padding: 8px;
		word-break: normal;
		overflow-wrap: anywhere;
	}

	.mobile .image-container img {
		max-height: calc(var(--preview-box-max) - 8px);
	}

	.mobile .empty-content,
	.mobile .loading-more {
		font-size: 16px;
		line-height: 20px;
	}

	.mobile .loading-more {
		padding: 6px 8px;
	}

	/* 2x2 grid of equal buttons: four in a row do not fit a phone width. */
	.mobile .actions {
		display: grid;
		grid-template-columns: repeat(2, minmax(0, 1fr));
		gap: 8px;
		margin-top: 12px;
	}

	.mobile .actions :global(.sys7-btn) {
		width: 100%;
		min-height: 44px;
		padding: 4px 8px 3px;
		white-space: nowrap;
	}

	/* Landscape phones have width to spare but little height: details and
	   buttons go left, the text gets the full height on the right. */
	@media (orientation: landscape) and (min-width: 600px) {
		.preview-content.mobile {
			--preview-box-max: calc(
				100vh - env(safe-area-inset-top, 0px) - env(safe-area-inset-bottom, 0px) - 96px
			);
			display: grid;
			grid-template-columns: 288px minmax(0, 1fr);
			grid-template-rows: auto 1fr;
			grid-template-areas:
				'meta content'
				'actions content';
			column-gap: 16px;
		}

		@supports (height: 100dvh) {
			.preview-content.mobile {
				--preview-box-max: calc(
					100dvh - env(safe-area-inset-top, 0px) - env(safe-area-inset-bottom, 0px) - 96px
				);
			}
		}

		.mobile .meta {
			grid-area: meta;
		}

		/* Five meta rows plus two button rows must fit a 360px-tall screen. */
		.mobile .meta-row {
			padding: 0;
		}

		.mobile .content-display {
			grid-area: content;
			margin-top: 0;
			min-height: 0;
		}

		.mobile .actions {
			grid-area: actions;
			align-self: end;
			margin-top: 8px;
		}
	}
</style>
