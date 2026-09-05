import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { createContext, runInContext } from 'node:vm';

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
