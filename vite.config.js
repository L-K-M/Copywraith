import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';
import bundledModules from './scripts/bundled-modules.mjs';

// Preserve Vite 6 output for older desktop webviews, including System7 CSS.
const desktopTargets = ['es2020', 'edge88', 'firefox78', 'chrome87', 'safari14'];

const host = process.env.TAURI_DEV_HOST;

// Bundled third-party code keeps its own licenses. bundledModules() records what
// ships for scripts/third-party-notices.mjs, which writes build/Third-Party-Notices.txt.
const banner =
	'/*! Bundled third-party code keeps its own licenses: see Third-Party-Notices.txt at the site or app root. */';
// Emit fonts as files, never data: URIs, so the notices check can attribute them.
/** @param {string} file */
const keepFontFiles = (file) => (/\.(?:woff2?|ttf|otf|eot)$/i.test(file) ? false : undefined);

export default defineConfig({
	plugins: [sveltekit(), bundledModules()],
	clearScreen: false,
	build: {
		target: desktopTargets,
		cssTarget: desktopTargets,
		assetsInlineLimit: keepFontFiles,
		// Rolldown adds postBanner after minification, so the comment survives.
		rolldownOptions: { output: { postBanner: banner } }
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
