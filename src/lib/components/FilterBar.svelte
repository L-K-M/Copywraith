<script lang="ts">
	import {
		filterText,
		starredOnly,
		debouncedLoad,
		moveSelection,
		pasteSelectedEntry
	} from '$lib/util/clipboardStore';
	import { Button, Checkbox } from '@lkmc/system7-ui';

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
				filterText.set('');
				debouncedLoad();
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
			placeholder="Filter clipboard..."
			value={$filterText}
			oninput={handleInput}
			onkeydown={handleKeydown}
		/>
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
	}

	.filter-input {
		width: 100%;
		box-sizing: border-box;
	}

	.filter-options {
		display: flex;
		align-items: center;
		gap: 8px;
		flex-shrink: 0;
		white-space: nowrap;
	}
</style>
