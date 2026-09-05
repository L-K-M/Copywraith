import { readFileSync } from 'node:fs';
import assert from 'node:assert/strict';
import { test } from 'node:test';

const workflow = readFileSync(new URL('../../.github/workflows/kde.yml', import.meta.url), 'utf8');

for (const desktop of ['plasma5', 'plasma6']) {
    test(`${desktop} runs mock security tests separately from runtime tests`, () => {
        const job = workflow.split(`  ${desktop}:\n`)[1]?.split(/\n  \w+:\n/)[0];
        assert.ok(job, `${desktop} job must exist`);
        assert.match(job, /run: scripts\/test-kde\.sh\s/);
        assert.match(job, /run: scripts\/test-kde-runtime\.sh\s/);
    });
}

test('Fedora uses pinned rustup rather than distro Rust', () => {
    const job = workflow.split('  plasma6:\n')[1];
    assert.doesNotMatch(job, /dnf install[^\n]*\b(?:cargo|rust)\b/);
    assert.match(job, /uses: dtolnay\/rust-toolchain@1\.98\.0/);
});

test('KDE workflow grants only read access to contents', () => {
    assert.match(workflow, /^permissions:\n  contents: read\n/m);
});
