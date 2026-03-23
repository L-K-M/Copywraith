<script lang="ts">
	import { filterText, starredOnly, debouncedLoad } from '$lib/util/clipboardStore';
	import { Checkbox } from '@lkmc/system7-ui';

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
		flex-shrink: 0;
		white-space: nowrap;
	}
</style>
