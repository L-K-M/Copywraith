import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import ts from 'typescript';

const source = readFileSync(new URL('../../src/lib/util/capturePause.ts', import.meta.url), 'utf8');
const module = { exports: {} };
new Function('module', 'exports', ts.transpileModule(source, {
	compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS }
}).outputText)(module, module.exports);
const { isPausedAt, pauseLabel, PAUSE_CHOICES } = module.exports;

const NOW = Date.parse('2026-09-25T12:00:00Z');
const inMinutes = (minutes) => new Date(NOW + minutes * 60_000).toISOString();

test('capturing shows no pause label', () => {
	const status = { paused: false, until: null };
	assert.equal(isPausedAt(status, NOW), false);
	assert.equal(pauseLabel(status, NOW), '');
});

test('an indefinite pause stays until resumed', () => {
	const status = { paused: true, until: null };
	assert.equal(isPausedAt(status, NOW + 10 * 86_400_000), true);
	assert.equal(pauseLabel(status, NOW), 'zzz Paused');
});

test('a timed pause counts down and ends by itself', () => {
	assert.equal(pauseLabel({ paused: true, until: inMinutes(5) }, NOW), 'zzz Paused 5m');
	assert.equal(pauseLabel({ paused: true, until: inMinutes(0.2) }, NOW), 'zzz Paused 1m');
	assert.equal(pauseLabel({ paused: true, until: inMinutes(60) }, NOW), 'zzz Paused 1h 00m');
	assert.equal(pauseLabel({ paused: true, until: inMinutes(95) }, NOW), 'zzz Paused 1h 35m');

	const expired = { paused: true, until: inMinutes(-1) };
	assert.equal(isPausedAt(expired, NOW), false);
	assert.equal(pauseLabel(expired, NOW), '');
});

test('the menu offers short, long and open-ended pauses', () => {
	assert.deepEqual(PAUSE_CHOICES.map((choice) => choice.minutes), [5, 60, null]);
});
