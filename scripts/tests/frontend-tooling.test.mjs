import assert from 'node:assert/strict';
import { test } from 'node:test';
import config from '../../vite.config.js';

// Keep desktop output compatible when Vite raises its default browser baseline.
const successExitCode = 0;
const typeErrorExitCode = 1;
const checkerTimeoutMs = 30_000;
const desktopTargets = ['es2020', 'edge88', 'firefox78', 'chrome87', 'safari14'];

test('desktop JavaScript and CSS retain the Vite 6 browser baseline', () => {
  assert.deepEqual(config.build?.target, desktopTargets);
  assert.deepEqual(config.build?.cssTarget, desktopTargets);
});

// A broken checker can exit successfully without reporting type errors.
test('TS7 alias reports Svelte type errors and accepts the corrected fixture', async () => {
  const { mkdtemp, writeFile, rm } = await import('node:fs/promises');
  const { spawnSync } = await import('node:child_process');
  const { fileURLToPath } = await import('node:url');
  const workspace = await mkdtemp(new URL('../../.tooling-test-', import.meta.url));
  const checker = fileURLToPath(new URL('../../node_modules/svelte-check/bin/svelte-check', import.meta.url));

  try {
    await writeFile(`${workspace}/svelte.config.js`, 'export default {};');
    await writeFile(`${workspace}/tsconfig.json`, JSON.stringify({
      compilerOptions: { strict: true, target: 'ESNext', module: 'ESNext', moduleResolution: 'bundler', skipLibCheck: true },
      include: ['*.svelte']
    }));

    for (const [value, expectedStatus] of [['"wrong"', typeErrorExitCode], ['42', successExitCode]]) {
      await writeFile(`${workspace}/Probe.svelte`, `<script lang="ts">let value: number = ${value};</script><p>{value}</p>`);
      const result = spawnSync(process.execPath, [checker, '--workspace', workspace, '--tsconfig', './tsconfig.json', '--tsgo'], { encoding: 'utf8', timeout: checkerTimeoutMs });
      const output = result.stdout + result.stderr;

      assert.equal(result.status, expectedStatus, output);
      if (expectedStatus === successExitCode) {
        assert.match(output, /0 errors and 0 warnings/);
        continue;
      }

      assert.match(output, /Type 'string' is not assignable to type 'number'/);
    }
  } finally {
    await rm(workspace, { recursive: true, force: true });
  }
});
