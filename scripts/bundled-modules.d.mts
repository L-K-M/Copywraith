// Types for the vendored bundled-modules.mjs, so a Vite config checked with
// checkJs (server/ui/tsconfig.json) does not type-check the vendored source.
export default function bundledModules(options?: { dir?: string }): { name: string; apply: 'build' };
