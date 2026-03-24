import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

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
		outDir: 'dist',
		emptyOutDir: true
	}
});
