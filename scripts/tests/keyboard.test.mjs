import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import ts from 'typescript';

const source = readFileSync(new URL('../../src/lib/util/keyboard.ts', import.meta.url), 'utf8');
const keyboard = { exports: {} };
new Function('module', 'exports', ts.transpileModule(source, {
	compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS }
}).outputText)(keyboard, keyboard.exports);
const { resolveShortcut, isModKey } = keyboard.exports;

const key = (overrides) => ({ key: '', metaKey: false, ctrlKey: false, shiftKey: false, altKey: false, ...overrides });
const inFilter = { platform: 'macos', inTextField: true, filterEmpty: true };

test('Mod+digit pastes that row, using Cmd on macOS and Ctrl elsewhere', () => {
	assert.deepEqual(resolveShortcut(key({ key: '3', metaKey: true }), inFilter), { type: 'quick-paste', index: 2 });
	assert.deepEqual(
		resolveShortcut(key({ key: '1', ctrlKey: true }), { ...inFilter, platform: 'linux' }),
		{ type: 'quick-paste', index: 0 }
	);
	// Ctrl on macOS and Cmd/Super elsewhere are not the modifier.
	assert.equal(resolveShortcut(key({ key: '3', ctrlKey: true }), inFilter), null);
	assert.equal(resolveShortcut(key({ key: '3', metaKey: true }), { ...inFilter, platform: 'linux' }), null);
	assert.equal(resolveShortcut(key({ key: '0', metaKey: true }), inFilter), null);
});

test('plain digits and letters in the filter stay text', () => {
	for (const typed of ['3', 's', 'y', 'f']) {
		assert.equal(resolveShortcut(key({ key: typed }), inFilter), null, typed);
	}
});

test('Mod+S stars, Mod+Y previews, Mod+F focuses the filter', () => {
	assert.deepEqual(resolveShortcut(key({ key: 's', metaKey: true }), inFilter), { type: 'toggle-star' });
	assert.deepEqual(resolveShortcut(key({ key: 'y', metaKey: true }), inFilter), { type: 'preview' });
	assert.deepEqual(resolveShortcut(key({ key: 'f', metaKey: true }), inFilter), { type: 'focus-filter' });
});

test('Mod+Backspace deletes only when it cannot be editing filter text', () => {
	const backspace = key({ key: 'Backspace', metaKey: true });
	assert.deepEqual(resolveShortcut(backspace, inFilter), { type: 'delete' });
	assert.deepEqual(resolveShortcut(backspace, { ...inFilter, inTextField: false, filterEmpty: false }), { type: 'delete' });
	assert.equal(resolveShortcut(backspace, { ...inFilter, filterEmpty: false }), null);
	assert.equal(resolveShortcut(key({ key: 'Backspace' }), inFilter), null);
});

test('Shift+Enter pastes as plain text', () => {
	assert.deepEqual(resolveShortcut(key({ key: 'Enter', shiftKey: true }), inFilter), { type: 'paste-plaintext' });
	assert.equal(resolveShortcut(key({ key: 'Enter' }), inFilter), null);
});

test('Page keys jump; Home and End only outside text fields', () => {
	assert.deepEqual(resolveShortcut(key({ key: 'PageDown' }), inFilter), { type: 'move', delta: 10 });
	assert.deepEqual(resolveShortcut(key({ key: 'PageUp' }), inFilter), { type: 'move', delta: -10 });
	assert.equal(resolveShortcut(key({ key: 'Home' }), inFilter), null);
	const list = { ...inFilter, inTextField: false };
	assert.deepEqual(resolveShortcut(key({ key: 'Home' }), list), { type: 'select-edge', edge: 'first' });
	assert.deepEqual(resolveShortcut(key({ key: 'End' }), list), { type: 'select-edge', edge: 'last' });
});

test('a held chord fires once, but held movement keys keep moving', () => {
	assert.equal(resolveShortcut(key({ key: 'Backspace', metaKey: true, repeat: true }), inFilter), null);
	assert.equal(resolveShortcut(key({ key: 's', metaKey: true, repeat: true }), inFilter), null);
	assert.equal(resolveShortcut(key({ key: 'Enter', shiftKey: true, repeat: true }), inFilter), null);
	assert.equal(resolveShortcut(key({ key: '1', metaKey: true, repeat: true }), inFilter), null);
	assert.deepEqual(resolveShortcut(key({ key: 'PageDown', repeat: true }), inFilter), { type: 'move', delta: 10 });
});

test('Option/Alt+Enter pastes as plain text, like Option/Alt+click', () => {
	assert.deepEqual(resolveShortcut(key({ key: 'Enter', altKey: true }), inFilter), { type: 'paste-plaintext' });
	assert.deepEqual(
		resolveShortcut(key({ key: 'Enter', altKey: true }), { ...inFilter, platform: 'linux' }),
		{ type: 'paste-plaintext' }
	);
});

test('Option/AltGr combinations are left to the keyboard layout', () => {
	assert.equal(resolveShortcut(key({ key: '2', metaKey: true, altKey: true }), inFilter), null);
	assert.equal(resolveShortcut(key({ key: '@', ctrlKey: true, altKey: true }), { ...inFilter, platform: 'linux' }), null);
});

test('the quick-paste modifier is Meta on macOS and Control elsewhere', () => {
	assert.equal(isModKey('Meta', 'macos'), true);
	assert.equal(isModKey('Control', 'macos'), false);
	assert.equal(isModKey('Control', 'linux'), true);
	assert.equal(isModKey('Meta', 'windows'), false);
});
