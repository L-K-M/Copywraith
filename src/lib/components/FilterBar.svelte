<script lang="ts">
	import {
		filterText,
		starredOnly,
		debouncedLoad,
		moveSelection,
		pasteSelectedEntry
	} from '$lib/util/clipboardStore';
	import { Button, Checkbox } from '@lkmc/system7-ui';
	import { isMobile } from '$lib/util/platform';

	let { onsettings }: { onsettings?: () => void } = $props();

	let filterInput: HTMLInputElement;

	export function focus() {
		filterInput?.focus();
		filterInput?.select();
	}

	function handleInput(e: Event) {
		const target = e.target as HTMLInputElement;
		filterText.set(target.value);
		debouncedLoad();
	}

	function clearFilter() {
		if (!$filterText) {
			return;
		}

		filterText.set('');
		debouncedLoad();
	}

	function handleClearFilter() {
		clearFilter();
		filterInput?.focus();
	}

	function handleStarredChange(checked: boolean) {
		starredOnly.set(checked);
		debouncedLoad();
	}

	function handleKeydown(e: KeyboardEvent) {
		if (e.key === 'ArrowDown') {
			e.preventDefault();
			moveSelection(1);
			return;
		}

		if (e.key === 'ArrowUp') {
			e.preventDefault();
			moveSelection(-1);
			return;
		}

		if (e.key === 'Enter') {
			e.preventDefault();
			pasteSelectedEntry();
			return;
		}

		if (e.key === 'Escape') {
			if ($filterText) {
				e.preventDefault();
				e.stopPropagation();
				clearFilter();
			}
		}
	}
</script>

<div class="filter-bar">
	<div class="filter-input-wrapper">
		<input
			bind:this={filterInput}
			type="text"
			class="s7-input filter-input"
			placeholder={$isMobile ? 'Filter...' : 'Filter clipboard...'}
			aria-label="Filter clipboard history"
			value={$filterText}
			oninput={handleInput}
			onkeydown={handleKeydown}
		/>
		{#if $filterText}
			<button
				type="button"
				class="clear-filter-btn"
				onclick={handleClearFilter}
				aria-label="Clear filter"
			>
				Clear
			</button>
		{/if}
	</div>
	<div class="filter-options">
		<Checkbox
			checked={$starredOnly}
			onchange={handleStarredChange}
		>
			Starred only
		</Checkbox>
		<Button onclick={onsettings}>Settings</Button>
	</div>
</div>

<style>
	.filter-bar {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 6px 8px;
		border-bottom: 1px solid #000;
		background: #fff;
	}

	.filter-input-wrapper {
		flex: 1;
		position: relative;
	}

	.filter-input {
		width: 100%;
		box-sizing: border-box;
		padding-right: 54px;
	}

	.clear-filter-btn {
		position: absolute;
		right: 6px;
		top: 50%;
		transform: translateY(-50%);
		height: 20px;
		min-width: 42px;
		padding: 0 6px;
		border: 1px solid #000;
		border-right-color: #666;
		border-bottom-color: #666;
		background: #ddd;
		font-size: 11px;
		line-height: 1;
		cursor: pointer;
	}

	.clear-filter-btn:active {
		border-right-color: #000;
		border-bottom-color: #000;
		border-left-color: #666;
		border-top-color: #666;
	}

	.filter-options {
		display: flex;
		align-items: center;
		gap: 8px;
		flex-shrink: 0;
		white-space: nowrap;
	}

	@media (max-width: 520px) {
		.filter-bar {
			flex-wrap: wrap;
			align-items: stretch;
			padding: 5px 6px;
			gap: 6px;
		}

		.filter-input-wrapper {
			flex: 1 1 100%;
			min-width: 0;
		}

		.filter-options {
			width: 100%;
			justify-content: space-between;
		}
	}
</style>
