<script lang="ts">
	import { MovableDialog, Button } from '@lkmc/system7-ui';
	import type { EntryResponse } from './types';

	let {
		entry,
		onclose,
		ondownload
	}: {
		entry: EntryResponse;
		onclose: () => void;
		ondownload: (entry: EntryResponse) => void;
	} = $props();

	function formatDate(iso: string | null): string {
		if (!iso) return '--';
		const d = new Date(iso);
		const pad = (n: number) => String(n).padStart(2, '0');
		return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
	}

	function formatSize(bytes: number | null): string {
		if (bytes == null) return '';
		if (bytes < 1024) return bytes + ' B';
		if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + ' KB';
		return (bytes / (1024 * 1024)).toFixed(1) + ' MB';
	}
</script>

<MovableDialog title="Entry: {entry.id.substring(0, 12)}..." {onclose} width="600px">
	<div class="detail-content">
		<div class="meta">
			<span>Type: <strong>{entry.content_type}</strong></span>
			<span>Starred: {entry.starred ? 'Yes' : 'No'}</span>
			<span>Created: {formatDate(entry.created_at)}</span>
			<span>Updated: {formatDate(entry.updated_at)}</span>
		</div>

		{#if entry.source_app}
			<div class="meta">
				<span>Source: {entry.source_app}</span>
			</div>
		{/if}

		{#if entry.sensitive}
			<div class="meta">
				<span class="sensitive-label">Sensitive: Yes</span>
			</div>
		{/if}

		{#if entry.sensitive}
			<div class="sensitive-placeholder">[Sensitive content hidden]</div>
		{:else if entry.content_type === 'image' && entry.blob_url}
			<img class="img-full" src={entry.blob_url} alt="Clipboard content preview" />
			{#if entry.blob_size}
				<div class="meta size-meta">
					<span>Size: {formatSize(entry.blob_size)}</span>
				</div>
			{/if}
		{:else if entry.text_content}
			<pre class="text-content">{entry.text_content}</pre>
		{:else}
			<div class="empty-state">No displayable content</div>
		{/if}

		<div class="actions">
			<Button onclick={() => ondownload(entry)}>Download</Button>
			<Button onclick={onclose}>Close</Button>
		</div>
	</div>
</MovableDialog>

<style>
	.detail-content {
		display: flex;
		flex-direction: column;
		gap: 8px;
		max-height: min(760px, calc(100dvh - 180px));
		overflow-y: auto;
		overflow-x: hidden;
		padding-right: 2px;
	}

	.meta {
		font-size: 11px;
		color: #888;
	}

	.meta span {
		display: inline-block;
		margin-right: 12px;
	}

	.meta strong {
		color: #000;
	}

	.size-meta {
		margin-top: 4px;
	}

	.img-full {
		max-width: 100%;
		max-height: min(56dvh, 620px);
		object-fit: contain;
		border: 1px solid #ccc;
	}

	.text-content {
		font-family: "Monaco", "Courier New", monospace;
		font-size: 11px;
		white-space: pre-wrap;
		word-break: break-all;
		background: #f5f5f5;
		border: 1px inset;
		padding: 8px;
		max-height: min(46dvh, 520px);
		overflow-y: auto;
		margin: 0;
	}

	.empty-state {
		text-align: center;
		padding: 32px;
		color: #888;
	}

	.sensitive-label {
		color: #c44;
		font-weight: bold;
	}

	.sensitive-placeholder {
		text-align: center;
		padding: 32px;
		color: #888;
		font-style: italic;
		background: #f5f5f5;
		border: 1px inset;
	}

	.actions {
		display: flex;
		gap: 8px;
		justify-content: flex-end;
		padding-top: 8px;
		border-top: 1px solid #ccc;
	}
</style>
