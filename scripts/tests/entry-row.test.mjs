import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import ts from 'typescript';

// Exercise the component's actual handler without requiring a desktop WebView.
const source = readFileSync(new URL('../../src/lib/components/EntryRow.svelte', import.meta.url), 'utf8');
const handler = source.slice(source.indexOf('\tfunction handleKeydown('), source.indexOf('\tfunction handleFocus('));
const javascript = ts.transpile(handler, { target: ts.ScriptTarget.ESNext });

for (const key of ['Enter', ' ']) {
  test(`${JSON.stringify(key)} leaves child button activation to the button`, () => {
    const calls = [];
    const run = new Function('pasteEntry', 'onpreview', 'onselect', 'entry', `${javascript}; return handleKeydown;`)(
      () => calls.push('paste'), () => calls.push('preview'), () => {}, { id: 'entry' }
    );
    run({ key, target: {}, currentTarget: {}, preventDefault: () => calls.push('prevent'), stopPropagation() {} });
    assert.deepEqual(calls, []);
  });

  test(`${JSON.stringify(key)} still activates the focused row`, () => {
    const calls = [];
    const row = {};
    const run = new Function('pasteEntry', 'onpreview', 'onselect', 'entry', `${javascript}; return handleKeydown;`)(
      () => calls.push('paste'), () => calls.push('preview'), () => {}, { id: 'entry' }
    );
    run({ key, target: row, currentTarget: row, preventDefault() {}, stopPropagation() {} });
    assert.deepEqual(calls, [key === 'Enter' ? 'paste' : 'preview']);
  });
}
