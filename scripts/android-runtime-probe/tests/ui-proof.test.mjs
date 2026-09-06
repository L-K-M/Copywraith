import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';

const helper = new URL('../androidTest/ui-proof.js', import.meta.url);
const downloaded = 'android-headless-download';

function proof(invoke, body = 'Copywraith', marker = 'true') {
    const context = vm.createContext({window: {__TAURI_INTERNALS__: {invoke}}, document: {
        documentElement: {dataset: {probe: marker}}, body: {innerText: body},
    }});
    vm.runInContext(fs.readFileSync(helper, 'utf8'), context);
    return {context, start: (token, expected) => context.window.runtimeProbe.start(token, expected),
        ready: token => context.window.runtimeProbe.ready(token)};
}

const replies = async command => command === 'get_platform' ? 'android' : [{full_text: downloaded}];

test('stale success cannot satisfy a new call with pending IPC', () => {
    const page = proof(() => new Promise(() => {}), downloaded);
    page.start('fresh-call', downloaded);
    assert.equal(page.ready('fresh-call'), false);
});

test('download in IPC alone is insufficient; it must be rendered', async () => {
    const page = proof(replies);
    await page.start('render-call', downloaded);
    assert.equal(page.ready('render-call'), false);
    page.context.document.body.innerText = downloaded;
    assert.equal(page.ready('render-call'), true);
});

test('empty fixture still requires successful IPC', async () => {
    const page = proof(replies);
    await page.start('empty-call', '');
    assert.equal(page.ready('empty-call'), true);
});

test('an older IPC completion cannot satisfy a newer call', async () => {
    let release;
    const pending = new Promise(resolve => { release = resolve; });
    const page = proof(command => command === 'get_platform' ? pending : [], 'Copywraith', '');
    const previous = page.start('old-call', '');
    page.start('new-call', downloaded);
    release('android');
    await previous;
    assert.equal(page.ready('old-call'), false);
    assert.equal(page.ready('new-call'), false);
});

test('a completed earlier call cannot satisfy a new pending call', async () => {
    let pending = false;
    const page = proof(command => pending ? new Promise(() => {}) : replies(command), downloaded);
    await page.start('completed-call', downloaded);
    assert.equal(page.ready('completed-call'), true);
    pending = true;
    page.start('pending-call', downloaded);
    assert.equal(page.ready('pending-call'), false);
    assert.equal(page.ready('completed-call'), false);
});
