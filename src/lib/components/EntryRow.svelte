<script lang="ts">
	import type { ClipboardEntry } from '$lib/types';
	import { toggleStar, pasteEntry, pasteEntryPlaintext, deleteEntry } from '$lib/util/clipboardStore';
	import { isMobile } from '$lib/util/platform';
	import { BalloonHelp } from '@lkmc/system7-ui';
	import { TauriService } from '$lib/tauri';
	import { now, formatRelativeTime, imageMimeFromBase64, RelativeTimeStyle } from '$lib/util/clock';

	/**
	 * Start fetching a row's image this far outside the viewport.
	 *
	 * Large enough that a normal scroll never shows an empty cell, small enough
	 * that opening the popup does not decode every image in the history.
	 */
	const IMAGE_PREFETCH_MARGIN_PX = 300;

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
		quickKey = null,
		onselect,
		onpreview
	}: {
		entry: ClipboardEntry;
		isFirst?: boolean;
		selected?: boolean;
		/** Digit shown while the quick-paste modifier is held, or null. */
		quickKey?: number | null;
		onselect?: (id: string) => void;
		onpreview?: (entry: ClipboardEntry) => void;
	} = $props();

	let rowElement: HTMLTableRowElement | null = $state(null);
	let imageData: string | null = $state(null);
	let imageLoading = $state(false);
	let imageFailed = $state(false);
	let imageVisible = $state(false);

	// Metadata refreshes replace entry objects without changing their image.
	let imageEntryId = $derived(entry.has_image ? entry.id : null);

	// The mobile time column fits about three characters, not a date.
	let relativeTime = $derived(
		formatRelativeTime(
			entry.updated_at,
			$now,
			$isMobile ? RelativeTimeStyle.Compact : RelativeTimeStyle.Date
		)
	);
	let imageSrc = $derived(
		imageData ? `data:${imageMimeFromBase64(imageData)};base64,${imageData}` : null
	);

	// Only fetch a row's image once the row is on (or near) screen. Loading every
	// image eagerly meant a page of screenshots pushed tens of megabytes of
	// base64 across the IPC bridge before the user had scrolled at all.
	$effect(() => {
		if (!entry.has_image || imageVisible || !rowElement) return;

		if (typeof IntersectionObserver === 'undefined') {
			imageVisible = true;
			return;
		}

		const observer = new IntersectionObserver(
			(observed) => {
				if (observed.some((observation) => observation.isIntersecting)) {
					imageVisible = true;
					observer.disconnect();
				}
			},
			{ rootMargin: `${IMAGE_PREFETCH_MARGIN_PX}px` }
		);
		observer.observe(rowElement);

		return () => observer.disconnect();
	});

	$effect(() => {
		const id = imageEntryId;
		if (!id || !imageVisible) return;

		let disposed = false;

		imageLoading = true;
		imageFailed = false;

		TauriService.getEntryImage(id)
			.then((data) => {
				if (disposed) return;
				imageData = data;
				imageFailed = data === null;
			})
			.catch((e) => {
				if (disposed) return;
				console.error('Failed to load entry image:', e);
				imageFailed = true;
			})
			.finally(() => {
				// The invoke cannot be cancelled, but a row scrolled out of view
				// (or replaced) must not overwrite the current row's state.
				if (!disposed) imageLoading = false;
			});

		return () => {
			disposed = true;
		};
	});

	$effect(() => {
		if (selected && rowElement) {
			rowElement.scrollIntoView({ block: isFirst ? 'start' : 'nearest' });
		}
	});

	function handleClick(e: MouseEvent) {
		const SINGLE_CLICK_COUNT = 1;

		// Later clicks in a double-click must not paste the same entry again.
		if (e.detail > SINGLE_CLICK_COUNT) return;

		onselect?.(entry.id);
		// On mobile, alt-click is not available; always do a standard paste/copy
		if (!$isMobile && e.altKey) {
			pasteEntryPlaintext(entry.id);
		} else {
			pasteEntry(entry.id);
		}
	}

	function handleImageDecodeError() {
		imageData = null;
		imageFailed = true;
	}

	function handlePreviewClick(e: MouseEvent) {
		e.preventDefault();
		e.stopPropagation();
		onselect?.(entry.id);
		onpreview?.(entry);
	}

	function handleKeydown(e: KeyboardEvent) {
		// Buttons handle their own keys; bubbling Enter must not paste the row.
		if (e.target !== e.currentTarget) return;

		if (e.key === 'Enter') {
			e.preventDefault();
			e.stopPropagation();
			// A held Enter must not paste over and over.
			if (e.repeat) return;
			onselect?.(entry.id);
			// Shift or Option/Alt, like Option/Alt+click.
			if (e.shiftKey || e.altKey) {
				pasteEntryPlaintext(entry.id);
			} else {
				pasteEntry(entry.id);
			}
		}
		// Space shows preview
		if (e.key === ' ') {
			e.preventDefault();
			e.stopPropagation();
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
<!--
	Note: there is deliberately no `ondblclick` here. A double-click emits
	`click, click, dblclick`, and `handleClick` pastes and hides the popup — so
	double-clicking to preview used to paste the entry into the target app twice
	before trying to show a dialog over a hidden window. Preview is reachable via
	the Space key and the preview button in the actions cell, which also gives
	touch devices a preview path they never had.
-->
<!--
	Selection exists for keyboard navigation, which mobile does not have, so a
	phone shows no persistent highlight; a tap flashes the row instead.
-->
<tr
	class="entry-row"
	class:selected={selected && !$isMobile}
	class:mobile={$isMobile}
	bind:this={rowElement}
	onclick={handleClick}
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
		{#if quickKey !== null}
			<span class="quick-key" aria-hidden="true">{quickKey}</span>
		{/if}
		{#if entry.has_image && imageSrc}
			<div class="image-preview">
				<!--
					A blob can be truncated, corrupt, or in a format this WebView
					cannot decode. Without onerror the cell would show a broken
					image icon forever, even though the fallback text state below
					already exists.
				-->
				<img
					src={imageSrc}
					alt="Copied screenshot"
					onerror={handleImageDecodeError}
				/>
			</div>
		{:else if entry.has_image && imageLoading}
			<div class="text-preview muted">[Loading image...]</div>
		{:else if entry.has_image && imageFailed}
			<div class="text-preview muted">[Image unavailable]</div>
		{:else if entry.has_image}
			<div class="text-preview muted">[Image]</div>
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
		{#if $isMobile}
			<!-- Balloon help needs hover; the preview dialog shows the full date. -->
			<span class="time">{relativeTime}</span>
		{:else}
			<BalloonHelp message={new Date(entry.updated_at).toLocaleString()} delay={600}>
				<span class="time">{relativeTime}</span>
			</BalloonHelp>
		{/if}
	</td>
	<td class="col-actions">
		<div class="row-actions">
			<button
				type="button"
				class="row-action-btn preview-btn"
				onmousedown={stopRowClick}
				onclick={handlePreviewClick}
				title="Preview"
				aria-label="Preview entry"
			>
				{'\u2026'}
			</button>
			<button
				type="button"
				class="row-action-btn delete-btn"
				onmousedown={stopRowClick}
				onclick={handleDeleteClick}
				title="Delete"
				aria-label="Delete entry"
			>
				<!-- Geneva has no U+2715; its own multiplication sign stays pixel-styled. -->
				{$isMobile ? '\u00d7' : '\u2715'}
			</button>
		</div>
	</td>
</tr>

<style>
	.entry-row {
		cursor: pointer;
		user-select: none;
	}

	/*
	 * Keep keyboard focus visible over selection and hover. Touch browsers
	 * keep :hover on the last tapped row, so hover styling needs a real hover.
	 */
	@media (hover: hover) {
		.entry-row:hover {
			background: var(--system7-color-highlight, #000);
			color: var(--system7-color-highlight-text, #fff);
		}

		.entry-row:hover .star-btn.starred {
			color: #ffd700;
		}
	}

	.entry-row.selected {
		background: var(--system7-color-highlight, #000);
		color: var(--system7-color-highlight-text, #fff);
	}

	.entry-row:focus-visible {
		outline: 2px solid var(--system7-color-highlight, #000);
		outline-offset: -2px;
	}

	.entry-row.selected:focus-visible {
		outline-color: var(--system7-color-highlight-text, #fff);
	}

	.col-star {
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

	.entry-row.selected .star-btn.starred,
	.entry-row.mobile:active .star-btn.starred {
		color: #ffd700;
	}

	.col-content {
		max-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		padding: 3px 6px;
	}

	/* A System 7 style keycap in front of the first nine rows. */
	.quick-key {
		float: left;
		min-width: 14px;
		margin: 5px 6px 0 0;
		padding: 0 2px;
		border: 1px solid currentColor;
		border-radius: 3px;
		box-shadow: 1px 1px 0 currentColor;
		font-size: 11px;
		line-height: 14px;
		text-align: center;
	}

	.text-preview {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-size: 24px;
	}

	.text-preview.muted {
		opacity: 0.6;
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
		text-align: right;
		padding: 2px 4px;
		font-size: 11px;
		opacity: 0.7;
	}

	.col-actions {
		text-align: right;
		padding: 2px 4px;
	}

	.row-actions {
		display: flex;
		align-items: center;
		justify-content: flex-end;
		gap: 2px;
	}

	.row-action-btn {
		background: none;
		border: none;
		cursor: pointer;
		font-size: 10px;
		line-height: 1;
		padding: 2px;
		color: inherit;
		opacity: 0;
		transition: opacity 0.12s ease;
	}

	/*
	 * Reveal on hover, on row focus, and whenever the row is selected — the
	 * previous hover-only rule left these controls permanently invisible to a
	 * keyboard user who could nonetheless tab straight to them.
	 */
	.entry-row:hover .row-action-btn,
	.entry-row:focus-within .row-action-btn,
	.entry-row.selected .row-action-btn {
		opacity: 0.6;
	}

	.row-action-btn:hover,
	.row-action-btn:focus-visible {
		opacity: 1;
	}

	@media (prefers-reduced-motion: reduce) {
		.row-action-btn {
			transition: none;
		}
	}

	/*
	 * Mobile. Copied text uses the same 24px as desktop (Geneva's 1.5x grid);
	 * the badge and age use its crisp 1x size, 16px. Anything smaller is
	 * illegible on a phone. Cells need two classes plus the row's to beat the
	 * library's (0,3,2) cell padding rule. The 44px star button sets the row
	 * height, so every control in the row is a comfortable touch target.
	 */
	.entry-row.mobile:active {
		background: var(--system7-color-highlight, #000);
		color: var(--system7-color-highlight-text, #fff);
	}

	.entry-row.mobile .col-star {
		padding: 0;
	}

	.entry-row.mobile .star-btn {
		display: block;
		width: 100%;
		height: 44px;
		padding: 0;
		font-size: 22px;
	}

	.entry-row.mobile .col-content {
		padding: 0 6px 0 2px;
	}

	.entry-row.mobile .text-preview {
		font-size: 24px;
	}

	.entry-row.mobile .image-preview {
		height: 56px;
	}

	.entry-row.mobile .image-preview img {
		max-width: 100%;
		max-height: 56px;
	}

	.entry-row.mobile .col-type {
		padding: 0 2px;
		text-align: center;
	}

	.entry-row.mobile .type-badge {
		padding: 0 2px;
		font-size: 16px;
	}

	.entry-row.mobile .col-time {
		padding: 0 4px;
		text-align: right;
	}

	.entry-row.mobile .time {
		font-size: 16px;
	}

	.entry-row.mobile .col-actions {
		padding: 0;
	}

	.entry-row.mobile .row-actions {
		gap: 0;
	}

	/* There is no hover on touch, so the controls are always visible. */
	.entry-row.mobile .row-action-btn {
		width: 32px;
		height: 44px;
		padding: 0;
		font-size: 32px;
		opacity: 0.7;
	}

	/* Geneva's ellipsis sits on the baseline; lift it to the row's centre line. */
	.entry-row.mobile .preview-btn {
		padding-bottom: 14px;
	}
</style>
