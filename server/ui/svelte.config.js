import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

/**
 * Explicit config so the build no longer relies on plugin defaults, and so
 * `svelte-check` knows how to preprocess the `lang="ts"` blocks in components.
 *
 * This is a plain Svelte SPA, not SvelteKit — there is no `kit` section here.
 */
export default {
	preprocess: vitePreprocess()
};
