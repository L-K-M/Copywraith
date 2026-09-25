import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { createContext, runInContext } from 'node:vm';
import ts from 'typescript';

// Exercise the component handler with backend outcomes, without a Tauri runtime.
const component = readFileSync(new URL('../../src/lib/components/StatusBar.svelte', import.meta.url), 'utf8');
const handler = component.slice(component.indexOf('async function handleSyncNow()'), component.indexOf('</script>'));

for (const state of ['unreachable', 'disabled', 'checking']) {
	test(`manual sync does not claim success when ${state}`, async () => {
		const context = createContext({
			isSyncing: false,
			lastSyncSummary: null,
			configuredLocalUrl: null,
			configuredVpnUrl: null,
			setSyncEndpointStatus() {},
			TauriService: { syncNow: async () => ({ pulled: 0, endpoint_status: { state } }) }
		});
		runInContext(handler, context);
		await context.handleSyncNow();
		assert.notEqual(context.lastSyncSummary, 'Already up to date.');
		assert.equal(context.isSyncing, false);
	});
}

for (const [pulled, expected] of [[0, 'No new entries pulled.'], [1, 'Pulled 1 entry.'], [2, 'Pulled 2 entries.']]) {
	test(`manual sync reports ${pulled} pulled entries`, async () => {
		const context = createContext({
			isSyncing: false,
			lastSyncSummary: null,
			configuredLocalUrl: null,
			configuredVpnUrl: null,
			setSyncEndpointStatus() {},
			TauriService: { syncNow: async () => ({ pulled, endpoint_status: { state: 'online' } }) }
		});
		runInContext(handler, context);
		await context.handleSyncNow();
		assert.equal(context.lastSyncSummary, expected);
	});
}

test('manual sync points at the password when the server rejects it', async () => {
	const context = createContext({
		isSyncing: false,
		lastSyncSummary: null,
		configuredLocalUrl: null,
		configuredVpnUrl: null,
		setSyncEndpointStatus() {},
		TauriService: { syncNow: async () => ({ pulled: 0, endpoint_status: { state: 'unauthorized' } }) }
	});
	runInContext(handler, context);
	await context.handleSyncNow();
	assert.match(context.lastSyncSummary, /password/);
});

// The store used to fold every unknown state into "unreachable", which would
// hide a rejected password behind a network message.
const store = readFileSync(new URL('../../src/lib/util/syncStatusStore.ts', import.meta.url), 'utf8');
const storeModule = { exports: {} };
new Function('require', 'module', 'exports', ts.transpileModule(store, {
	compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS }
}).outputText)(
	(name) => {
		assert.equal(name, 'svelte/store');
		return { writable: () => ({ set() {}, update() {} }) };
	},
	storeModule,
	storeModule.exports
);

for (const state of ['unauthorized', 'error', 'online', 'unreachable']) {
	test(`status store keeps the ${state} state`, () => {
		assert.equal(storeModule.exports.normalizeSyncEndpointStatus({ state }).state, state);
	});
}

test('status store treats an unknown state as unreachable', () => {
	assert.equal(storeModule.exports.normalizeSyncEndpointStatus({ state: 'haunted' }).state, 'unreachable');
});
