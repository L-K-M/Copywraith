<script lang="ts">
	import { entries, starredOnly } from '$lib/util/clipboardStore';

	let entryCount = $derived($entries.length);
	let starredLabel = $derived($starredOnly ? ' (starred)' : '');
</script>

<div class="status-bar">
	<span class="status-text">
		{entryCount} item{entryCount !== 1 ? 's' : ''}{starredLabel}
	</span>
	<span class="status-hint">
		Click to paste &middot; Opt+Click plaintext &middot; ↑/↓ select &middot; Enter paste
	</span>
</div>

<style>
	.status-bar {
		display: flex;
		justify-content: space-between;
		align-items: center;
		padding: 3px 8px;
		border-top: 1px solid #000;
		background: #eee;
		font-size: 10px;
		user-select: none;
		flex-shrink: 0;
	}

	.status-text {
		font-weight: bold;
	}

	.status-hint {
		opacity: 0.6;
	}
</style>
