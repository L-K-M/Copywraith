import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
	plugins: [sveltekit()],
	resolve: {
		alias: [
			{
				find: '@lkmc/system7-ui/styles.css',
				replacement: new URL('../system7-ui/src/styles/system7.css', import.meta.url).pathname
			},
			{
				find: '@lkmc/system7-ui',
				replacement: new URL('../system7-ui/src/index.ts', import.meta.url).pathname
			}
		]
	},
	clearScreen: false,
	server: {
		port: 1420,
		strictPort: true,
		host: host || false,
		hmr: host
			? {
					protocol: 'ws',
					host,
					port: 1421
				}
			: undefined,
		watch: {
			ignored: ['**/src-tauri/**']
		},
		fs: {
			allow: ['..']
		}
	}
});
