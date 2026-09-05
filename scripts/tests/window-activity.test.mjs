import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { setImmediate } from 'node:timers/promises';
import { test } from 'node:test';
import ts from 'typescript';

const ACTIVITY_CHANGED = 'window-activity-changed';
const KEYBOARD_FOCUS_CHANGED = 'keyboard-focus-changed';
const AUTO_HIDE_WAIT_MS = 1_000;
const page = readFileSync(new URL('../../src/routes/+page.svelte', import.meta.url), 'utf8');
const script = page.slice(page.indexOf('>') + 1, page.indexOf('</script>'));
const manager = readFileSync(new URL('../../src/lib/windowManager.ts', import.meta.url), 'utf8');

// Execute the actual handlers with native APIs stubbed, like the other popup tests.
function load(source, modules, bindings = {}) {
	const javascript = ts.transpileModule(source, {
		compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS }
	}).outputText;
	const exports = {};
	const require = (name) => {
		assert.ok(name in modules, `Unexpected import: ${name}`);
		return modules[name];
	};
	new Function('require', 'exports', ...Object.keys(bindings), javascript)(
		require, exports, ...Object.values(bindings)
	);
	return exports;
}

function popup(t, platformName = 'linux') {
	t.mock.timers.enable({ apis: ['setTimeout'] });
	const listeners = new Map();
	const mounts = [];
	const destroys = [];
	const state = { active: true, nativeActive: true, hidden: 0, captured: 0 };
	const listen = async (event, callback) => {
		listeners.set(event, callback);
		return () => { listeners.delete(event); };
	};
	const emit = (event, payload) => listeners.get(event)?.({ payload });
	const keyboardFocus = (focused) => {
		emit(KEYBOARD_FOCUS_CHANGED, focused);
		if (platformName !== 'linux') emit(ACTIVITY_CHANGED, focused);
	};
	const appWindow = {
		listen,
		onFocusChanged: (callback) => listen(KEYBOARD_FOCUS_CHANGED, callback),
		startDragging: async () => { keyboardFocus(false); }
	};
	const TauriService = {
		getPlatform: async () => platformName,
		hidePopup: async () => { state.hidden++; },
		getSettings: async () => ({}),
		captureClipboard: async () => { state.captured++; return false; },
		syncNow: async () => ({ pulled: 0, endpoint_status: { state: 'disabled' } }),
		hasPendingShares: async () => ({ pending: false }),
		onShizukuClipboardStaged: async () => ({ unregister: async () => {} })
	};
	const modules = {
		'@tauri-apps/api/window': { getCurrentWindow: () => appWindow },
		'@tauri-apps/api/event': { listen },
		'@tauri-apps/api/core': { invoke: async (command) => {
			assert.equal(command, 'is_window_active');
			return state.nativeActive;
		} },
		'$lib/tauri': { TauriService },
		'$lib/util/windowState': { windowFocused: { set: (active) => { state.active = active; } } },
		'$lib/util/platform': { platform: { set() {} } },
		'$lib/util/syncStatusStore': { setSyncEndpointStatus() {} },
		'$lib/util/clipboardStore': { loadEntries: async () => {} },
		'$lib/util/notifications': { notify() {} },
		svelte: { onMount: (callback) => mounts.push(callback), onDestroy: (callback) => destroys.push(callback) }
	};
	modules['$lib/windowManager'] = load(manager, modules);
	const handlers = load(`${script}\nexport { handleWindowDrag };`, modules, {
		$state: (value) => value,
		$derived: (value) => value,
		$platform: platformName
	});
	t.after(async () => {
		for (const destroy of destroys) destroy();
		await setImmediate();
		assert.equal(listeners.size, 0, 'native listeners must be removed on unmount');
	});
	return {
		state, emit, keyboardFocus,
		drag: handlers.handleWindowDrag,
		mount: async () => {
			for (const mount of mounts) await mount();
			await setImmediate();
		}
	};
}

test('inactive startup initializes decorations without auto-hiding', async (t) => {
	const app = popup(t);
	app.state.nativeActive = false;
	await app.mount();

	assert.equal(app.state.active, false);
	t.mock.timers.tick(AUTO_HIDE_WAIT_MS);
	assert.equal(app.state.hidden, 0, 'startup must not dismiss an already-hidden popup');
});

test('Linux move grabs leave title-bar decorations active', async (t) => {
	const app = popup(t);
	await app.mount();
	app.drag();
	assert.equal(app.state.active, true);
});

test('Linux move grabs do not auto-hide a dragged popup', async (t) => {
	const app = popup(t);
	await app.mount();
	app.drag();
	t.mock.timers.tick(AUTO_HIDE_WAIT_MS);
	assert.equal(app.state.hidden, 0);
});

test('real deactivation during a move grab still dims and auto-hides', async (t) => {
	const app = popup(t);
	await app.mount();
	app.drag();
	t.mock.timers.tick(AUTO_HIDE_WAIT_MS);
	app.emit(ACTIVITY_CHANGED, false);
	assert.equal(app.state.active, false);
	assert.equal(app.state.hidden, 0, 'auto-hide retains its grace period');
	t.mock.timers.tick(AUTO_HIDE_WAIT_MS);
	assert.equal(app.state.hidden, 1);
});

test('reactivation cancels pending auto-hide', async (t) => {
	const app = popup(t);
	await app.mount();
	app.emit(ACTIVITY_CHANGED, false);
	assert.equal(app.state.active, false);
	app.emit(ACTIVITY_CHANGED, true);
	t.mock.timers.tick(AUTO_HIDE_WAIT_MS);
	assert.equal(app.state.active, true);
	assert.equal(app.state.hidden, 0);
});

function deferred() {
	let resolve;
	let reject;
	const promise = new Promise((done, fail) => { resolve = done; reject = fail; });
	return { promise, resolve, reject };
}

function activitySubscription(t) {
	const registration = deferred();
	const snapshot = deferred();
	const updates = [];
	const state = { callback: null, removed: 0, queries: 0 };
	const { WindowManager } = load(manager, {
		'@tauri-apps/api/window': { getCurrentWindow: () => ({ listen: (event, callback) => {
			assert.equal(event, ACTIVITY_CHANGED);
			state.callback = callback;
			return registration.promise;
		} }) },
		'@tauri-apps/api/core': { invoke: (command) => {
			assert.equal(command, 'is_window_active');
			state.queries++;
			return snapshot.promise;
		} },
		'$lib/tauri': { TauriService: {} }
	});
	const stop = new WindowManager().subscribeActivity((active) => updates.push(active));
	t.after(() => { if (!state.removed) stop(); });
	return {
		state, snapshot, updates, stop, registration,
		register: () => registration.resolve(() => { state.removed++; })
	};
}

test('activity reads initial state after subscribing', async (t) => {
	const subscription = activitySubscription(t);
	assert.equal(subscription.state.queries, 0);
	subscription.register();
	subscription.snapshot.resolve(false);
	await setImmediate();
	assert.equal(subscription.state.queries, 1);
	assert.deepEqual(subscription.updates, [false]);
});

test('an activity event beats a stale initial snapshot', async (t) => {
	const subscription = activitySubscription(t);
	subscription.register();
	await setImmediate();
	subscription.state.callback({ payload: false });
	subscription.snapshot.resolve(true);
	await setImmediate();
	assert.deepEqual(subscription.updates, [false]);
});

test('late listener registration is removed after disposal', async (t) => {
	const subscription = activitySubscription(t);
	subscription.stop();
	subscription.register();
	subscription.state.callback({ payload: true });
	await setImmediate();
	assert.equal(subscription.state.removed, 1);
	assert.equal(subscription.state.queries, 0);
	assert.deepEqual(subscription.updates, []);
});

test('late snapshots and queued events are ignored after disposal', async (t) => {
	const subscription = activitySubscription(t);
	subscription.register();
	await setImmediate();
	subscription.stop();
	subscription.snapshot.resolve(false);
	subscription.state.callback({ payload: true });
	await setImmediate();
	assert.equal(subscription.state.removed, 1);
	assert.deepEqual(subscription.updates, []);
});

test('snapshot failure leaves the activity listener usable', async (t) => {
	const report = t.mock.method(console, 'error', () => {});
	const subscription = activitySubscription(t);
	subscription.register();
	subscription.snapshot.reject(new Error('snapshot failed'));
	await setImmediate();
	subscription.state.callback({ payload: false });
	assert.deepEqual(subscription.updates, [false]);
	assert.equal(report.mock.calls.length, 1);
});

test('listener registration failure is caught', async (t) => {
	const report = t.mock.method(console, 'error', () => {});
	const subscription = activitySubscription(t);
	subscription.registration.reject(new Error('registration failed'));
	await setImmediate();
	assert.equal(subscription.state.queries, 0);
	assert.equal(report.mock.calls.length, 1);
});

for (const platform of ['macos', 'windows']) {
	test(`${platform} focus still drives decorations and auto-hide`, async (t) => {
		const app = popup(t, platform);
		await app.mount();
		app.keyboardFocus(false);
		assert.equal(app.state.active, false);
		t.mock.timers.tick(AUTO_HIDE_WAIT_MS);
		assert.equal(app.state.hidden, 1);
	});
}

for (const platform of ['android', 'ios']) {
	test(`${platform} resume still refreshes clipboard without auto-hide`, async (t) => {
		const app = popup(t, platform);
		await app.mount();
		const captures = app.state.captured;
		assert.equal(captures, 1, 'initial mobile refresh must complete');

		app.keyboardFocus(false);
		t.mock.timers.tick(AUTO_HIDE_WAIT_MS);
		assert.equal(app.state.hidden, 0);
		app.keyboardFocus(true);
		await setImmediate();
		assert.equal(app.state.captured, captures + 1);
		assert.equal(app.state.hidden, 0);
	});
}
