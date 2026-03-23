<script lang="ts">
	import '../app.css';
	import '@lkmc/system7-ui/styles.css';
	import type { Snippet } from 'svelte';

	let { children }: { children: Snippet } = $props();
</script>

{@render children()}
