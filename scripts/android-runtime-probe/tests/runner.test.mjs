import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import test from 'node:test';
import {fileURLToPath} from 'node:url';

const runner = fileURLToPath(new URL('../run.mjs', import.meta.url));
const emulator = 'emulator-5554';

function run(mode, serial = emulator) {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'android-runner-test-'));
    const bin = path.join(root, 'bin');
    fs.mkdirSync(bin);
    const apkRoot = path.join(root, 'src-tauri/gen/android/app/build/outputs/apk');
    for (const apk of ['universal/debug/app-universal-debug.apk', 'androidTest/universal/debug/app-universal-debug-androidTest.apk']) {
        fs.mkdirSync(path.dirname(path.join(apkRoot, apk)), {recursive: true});
        fs.writeFileSync(path.join(apkRoot, apk), 'fake apk');
    }
    const log = path.join(root, 'calls.jsonl');
    fs.writeFileSync(path.join(bin, 'adb'), `#!${process.execPath}
const fs = require('node:fs');
let args = process.argv.slice(2);
fs.appendFileSync(process.env.FAKE_LOG, JSON.stringify(args)+'\\n');
if (args[0] === '-s') args = args.slice(2);
const command = args.join(' ');
const mode = process.env.FAKE_MODE;
const mapping = process.env.FAKE_LOG + '.mapping';
if (command === 'get-state') {
    if (mode === 'command-timeout') { console.log('checking selected emulator'); setTimeout(() => process.exit(0), 1000); }
    else console.log('device');
}
else if (command === 'shell getprop ro.kernel.qemu') console.log(process.env.FAKE_MODE === 'physical' ? '0' : '1');
else if (command === 'reverse --list') {
    if (mode === 'existing-mapping') console.log('emulator-5554 tcp:18763 tcp:9999');
    else if (fs.existsSync(mapping)) console.log(fs.readFileSync(mapping, 'utf8'));
}
else if (command === 'reverse --no-rebind tcp:18763 tcp:18763') {
    if (mode === 'mapping-race') { fs.writeFileSync(mapping, 'emulator-5554 tcp:18763 tcp:9999'); process.exit(1); }
    fs.writeFileSync(mapping, 'emulator-5554 tcp:18763 tcp:18763');
}
else if (command === 'reverse --remove tcp:18763') fs.unlinkSync(mapping);
else if (command.startsWith('shell am instrument')) {
    if (mode === 'mapping-replaced') fs.writeFileSync(mapping, 'emulator-5554 tcp:18763 tcp:9999');
    if (process.env.FAKE_MODE === 'instrumentation-timeout') { console.log('partial test progress'); setTimeout(() => process.exit(0), 1000); }
    else console.log('OK (1 test)');
}
`, {mode: 0o755});
    fs.writeFileSync(path.join(bin, 'cargo'), `#!${process.execPath}
if (process.env.FAKE_MODE === 'fixture-failure') { console.error('fixture failed'); process.exit(42); }
console.log('ANDROID_PROBE_FIXTURE_READY');
setInterval(() => {}, 1000);
`, {mode: 0o755});
    const env = {...process.env, PATH: bin, FAKE_LOG: log, FAKE_MODE: mode, ANDROID_PROBE_COMMAND_TIMEOUT_MS: '200', ANDROID_PROBE_INSTRUMENTATION_TIMEOUT_MS: '200'};
    if (serial === null) delete env.ANDROID_SERIAL;
    else env.ANDROID_SERIAL = serial;
    const result = spawnSync(process.execPath, [runner], {cwd: root, env, encoding: 'utf8', timeout: 5000});
    const calls = fs.existsSync(log) ? fs.readFileSync(log, 'utf8').trim().split('\n').filter(Boolean).map(JSON.parse) : [];
    const fixture = path.join(root, 'artifacts/android-runtime/fixture.log');
    const fixtureLog = fs.existsSync(fixture) ? fs.readFileSync(fixture, 'utf8') : '';
    const failure = path.join(root, 'artifacts/android-runtime/adb-failure.txt');
    const failureLog = fs.existsSync(failure) ? fs.readFileSync(failure, 'utf8') : '';
    const instrumentation = path.join(root, 'artifacts/android-runtime/instrumentation.txt');
    const instrumentationLog = fs.existsSync(instrumentation) ? fs.readFileSync(instrumentation, 'utf8') : '';
    fs.rmSync(root, {recursive: true, force: true});
    return {...result, calls, fixtureLog, failureLog, instrumentationLog};
}

function mutations(calls) {
    return calls.map(args => args[0] === '-s' ? args.slice(2) : args).filter(args =>
        ['install', 'uninstall'].includes(args[0]) || args[0] === 'reverse' && args[1] !== '--list' ||
        args[0] === 'shell' && ['pm grant', 'am instrument'].some(command => args.slice(1).join(' ').startsWith(command)));
}

test('requires explicit device selection before mutation', () => {
    const result = run('success', null);
    assert.notEqual(result.status, 0);
    assert.deepEqual(mutations(result.calls), []);
});

test('refuses physical devices before mutation', () => {
    const result = run('physical', 'physical-device');
    assert.notEqual(result.status, 0);
    assert.deepEqual(mutations(result.calls), []);
});

test('refuses an existing reverse mapping without changing it', () => {
    const result = run('existing-mapping');
    assert.notEqual(result.status, 0);
    assert.deepEqual(mutations(result.calls), []);
});

test('failed fixture startup retains logs and removes no reverse mapping', () => {
    const result = run('fixture-failure');
    assert.notEqual(result.status, 0);
    assert.deepEqual(mutations(result.calls), []);
    assert.match(result.fixtureLog, /fixture failed/);
});

test('successful execution selects the emulator for every adb command', () => {
    const result = run('success');
    assert.equal(result.status, 0, result.stderr);
    assert.ok(result.calls.every(args => args[0] === '-s' && args[1] === emulator));
    assert.ok(result.calls.some(args => args.includes('--remove')));
});


test('verifies emulator identity even for an emulator-shaped serial', () => {
    const result = run('physical');
    assert.notEqual(result.status, 0);
    assert.deepEqual(mutations(result.calls), []);
});

test('bounds instrumentation and retains partial diagnostics', () => {
    const result = run('instrumentation-timeout');
    assert.notEqual(result.status, 0);
    assert.match(result.failureLog, /ETIMEDOUT/);
    assert.match(result.instrumentationLog, /partial test progress/);
});


test('bounds ordinary adb commands before mutation', () => {
    const result = run('command-timeout');
    assert.notEqual(result.status, 0);
    assert.deepEqual(mutations(result.calls), []);
    assert.match(result.failureLog, /ETIMEDOUT/);
});

test('does not remove a mapping that raced its reservation', () => {
    const result = run('mapping-race');
    assert.notEqual(result.status, 0);
    assert.ok(!result.calls.some(args => args.includes('--remove')));
});

test('does not remove a mapping replaced after its own installation', () => {
    const result = run('mapping-replaced');
    assert.equal(result.status, 0, result.stderr);
    assert.ok(!result.calls.some(args => args.includes('--remove')));
});
