import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// Freeze the existing Vite 8 baseline across future tooling upgrades.
const browserTargets = ['chrome111', 'edge111', 'firefox114', 'safari16.4'];

export default defineConfig({
	plugins: [svelte()],
	server: {
		port: 4174,
		strictPort: true,
		proxy: {
			'/api': {
				target: 'http://localhost:3742',
				changeOrigin: true
			}
		}
	},
	build: {
		target: browserTargets,
		cssTarget: browserTargets,
		outDir: 'dist',
		emptyOutDir: true
	}
});
