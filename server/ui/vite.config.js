import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import bundledModules from '../../scripts/bundled-modules.mjs';

// Freeze the existing Vite 8 baseline across future tooling upgrades.
const browserTargets = ['chrome111', 'edge111', 'firefox114', 'safari16.4'];

// Bundled third-party code keeps its own licenses. bundledModules() records what
// ships for scripts/third-party-notices.mjs, which writes dist/Third-Party-Notices.txt.
const banner =
	'/*! Bundled third-party code keeps its own licenses: see Third-Party-Notices.txt at the site or app root. */';
// Emit fonts as files, never data: URIs, so the notices check can attribute them.
/** @param {string} file */
const keepFontFiles = (file) => (/\.(?:woff2?|ttf|otf|eot)$/i.test(file) ? false : undefined);

export default defineConfig({
	plugins: [svelte(), bundledModules()],
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
		emptyOutDir: true,
		assetsInlineLimit: keepFontFiles,
		// Rolldown adds postBanner after minification, so the comment survives.
		rolldownOptions: { output: { postBanner: banner } }
	}
});
