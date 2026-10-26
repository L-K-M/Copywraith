<script lang="ts">
	import { Button, MovableDialog } from '@lkmc/system7-ui';

	let { onclose }: { onclose: () => void } = $props();

	// Both files are generated into the frontend build (package.json "notices:*"
	// scripts), so every build that embeds this page also embeds them.
	const notices = [
		{ label: 'Third-party notices', src: '/Third-Party-Notices.txt' },
		{ label: 'Rust standard library', src: '/Rust-Licenses.html' }
	];
	let selected = $state(notices[0].src);
</script>

<MovableDialog title="Open-Source Licenses" {onclose} width="520px">
	<div class="licenses">
		<p class="licenses-intro">
			Copywraith's own code is public domain (Unlicense). It bundles third-party components that keep
			their own licenses; their license and notice texts follow.
		</p>
		<div class="licenses-tabs">
			{#each notices as notice (notice.src)}
				<Button
					variant={selected === notice.src ? 'primary' : 'default'}
					onclick={() => (selected = notice.src)}
				>
					{notice.label}
				</Button>
			{/each}
		</div>
		<iframe class="licenses-text" title="License texts" src={selected}></iframe>
		<div class="licenses-actions">
			<Button onclick={onclose}>Close</Button>
		</div>
	</div>
</MovableDialog>

<style>
	.licenses {
		display: flex;
		flex-direction: column;
		gap: 8px;
		padding: 8px 0;
	}

	.licenses-intro {
		margin: 0;
		font-size: 11px;
		line-height: 1.35;
	}

	.licenses-tabs,
	.licenses-actions {
		display: flex;
		gap: 8px;
	}

	.licenses-actions {
		justify-content: flex-end;
	}

	.licenses-text {
		width: 100%;
		height: 300px;
		border: 1px solid #000;
		background: #fff;
	}
</style>
