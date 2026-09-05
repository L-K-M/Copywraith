import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import assert from 'node:assert/strict';
import { test } from 'node:test';

test('Xvfb startup failure prints diagnostics before deleting its session', async () => {
    const directory = await mkdtemp(join(tmpdir(), 'kde-startup-'));
    const diagnostic = 'injected Xvfb startup failure';
    try {
        await writeFile(join(directory, 'Xvfb'), `#!/bin/sh\necho '${diagnostic}' >&2\nexit 1\n`, { mode: 0o755 });
        // Startup diagnostics need no bus; preserve only the launcher's argument flow.
        await writeFile(join(directory, 'dbus-run-session'), '#!/bin/sh\nshift 3\nexec "$@"\n', { mode: 0o755 });
        const result = spawnSync('bash', ['scripts/test-kde-runtime.sh'], {
            env: { ...process.env, PATH: `${directory}:${process.env.PATH}` },
            encoding: 'utf8', timeout: 15000,
        });
        assert.equal(result.error, undefined);
        assert.notEqual(result.status, 0);
        assert.ok(result.stderr.includes(diagnostic), result.stderr);
    } finally {
        await rm(directory, { recursive: true, force: true });
    }
});
