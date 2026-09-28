import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import ts from 'typescript';

const source = readFileSync(new URL('../../src/lib/util/clock.ts', import.meta.url), 'utf8');
const clock = { exports: {} };
// Only the shared `now` store needs svelte; the formatter under test does not.
const require = (name) => {
	assert.equal(name, 'svelte/store', `Unexpected import: ${name}`);
	return { readable: () => ({}) };
};
new Function('module', 'exports', 'require', ts.transpileModule(source, {
	compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS }
}).outputText)(clock, clock.exports, require);
const { formatRelativeTime, RelativeTimeStyle } = clock.exports;

const NOW = Date.parse('2026-09-28T12:00:00Z');
const DAY_MS = 24 * 60 * 60 * 1000;
const ago = (ms) => new Date(NOW - ms).toISOString();

test('recent ages are the same in both styles', () => {
	for (const style of [RelativeTimeStyle.Date, RelativeTimeStyle.Compact]) {
		assert.equal(formatRelativeTime(ago(30 * 1000), NOW, style), 'now');
		assert.equal(formatRelativeTime(ago(5 * 60 * 1000), NOW, style), '5m');
		assert.equal(formatRelativeTime(ago(23 * 60 * 60 * 1000), NOW, style), '23h');
		assert.equal(formatRelativeTime(ago(29 * DAY_MS), NOW, style), '29d');
	}
});

test('older entries default to a locale date', () => {
	const timestamp = ago(45 * DAY_MS);
	assert.equal(formatRelativeTime(timestamp, NOW), new Date(timestamp).toLocaleDateString());
});

test('the compact style keeps older entries within a narrow column', () => {
	assert.equal(formatRelativeTime(ago(30 * DAY_MS), NOW, RelativeTimeStyle.Compact), '1mo');
	assert.equal(formatRelativeTime(ago(364 * DAY_MS), NOW, RelativeTimeStyle.Compact), '12mo');
	assert.equal(formatRelativeTime(ago(365 * DAY_MS), NOW, RelativeTimeStyle.Compact), '1y');
	assert.equal(formatRelativeTime(ago(3 * 365 * DAY_MS), NOW, RelativeTimeStyle.Compact), '3y');
});

test('an unparseable timestamp is an em dash, not NaN', () => {
	assert.equal(formatRelativeTime('not a date', NOW, RelativeTimeStyle.Compact), '—');
});
