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

for (const altKey of [false, true]) {
  test(`double-click pastes once (alt=${altKey})`, () => {
    const calls = [];
    const clickHandler = source.slice(source.indexOf('\tfunction handleClick('), source.indexOf('\tfunction handleImageDecodeError('));
    const run = new Function('pasteEntry', 'pasteEntryPlaintext', 'onselect', 'entry', '$isMobile',
      `${ts.transpile(clickHandler)}; return handleClick;`)(
      () => calls.push('paste'), () => calls.push('plaintext'), () => {}, { id: 'entry' }, false
    );
    run({ detail: 1, altKey });
    run({ detail: 2, altKey });
    assert.deepEqual(calls, [altKey ? 'plaintext' : 'paste']);
    run({ detail: 1, altKey });
    assert.equal(calls.length, 2, 'a later independent click still pastes');
  });
}

// Compile the actual image state/effect with Svelte so dependency tracking is tested.
test('refreshing metadata retains the image; changing identity fetches again', async () => {
  const { compileModule } = await import('svelte/compiler');
  const { effect_root, flush } = await import('svelte/internal/client');
  const { mkdtemp, writeFile, rm } = await import('node:fs/promises');
  const directory = await mkdtemp(new URL('./image-test-', import.meta.url));
  let dispose;
  try {
    const declarations = source.slice(source.indexOf('\tlet imageData:'), source.indexOf('\tlet relativeTime'));
    const effectStart = source.indexOf('\t$effect(() => {', source.indexOf('observer.observe(rowElement);'));
    const effectEnd = source.indexOf('\n\t$effect(() => {', effectStart + 1);
    const module = `export function create(TauriService) {
      let entry = $state({ id: 'image-a', has_image: true, starred: false });
      ${declarations}
      imageVisible = true;
      ${source.slice(effectStart, effectEnd)}
      return (replacement) => { entry = replacement; };
    }`;
    const compiled = compileModule(ts.transpile(module, { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext }), { filename: 'image.svelte.js' });
    const path = `${directory}/image.mjs`;
    await writeFile(path, compiled.js.code);
    const { create } = await import(path);
    const calls = [];
    let replace;
    dispose = effect_root(() => {
      replace = create({ getEntryImage: async (id) => { calls.push(id); return 'image'; } });
    });
    flush();
    assert.deepEqual(calls, ['image-a']);
    flush(() => replace({ id: 'image-a', has_image: true, starred: true }));
    assert.deepEqual(calls, ['image-a']);
    flush(() => replace({ id: 'image-b', has_image: true, starred: false }));
    assert.deepEqual(calls, ['image-a', 'image-b']);
  } finally {
    dispose?.();
    await rm(directory, { recursive: true, force: true });
  }
});
