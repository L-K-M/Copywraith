import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

// Preserve Vite 6 output for older desktop webviews, including System7 CSS.
const desktopTargets = ['es2020', 'edge88', 'firefox78', 'chrome87', 'safari14'];

const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
	plugins: [sveltekit()],
	clearScreen: false,
	build: {
		target: desktopTargets,
		cssTarget: desktopTargets
	},
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
