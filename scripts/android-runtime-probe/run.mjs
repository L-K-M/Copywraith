import fs from 'node:fs';
import path from 'node:path';
import {spawn, spawnSync} from 'node:child_process';

const packageName = 'ch.lkmc.copywraith';
const fixturePort = 'tcp:18763';
const output = path.resolve('artifacts/android-runtime');
const apkRoot = 'src-tauri/gen/android/app/build/outputs/apk';
const appApk = `${apkRoot}/universal/debug/app-universal-debug.apk`;
const testApk = `${apkRoot}/androidTest/universal/debug/app-universal-debug-androidTest.apk`;
const fixtureReady = 'ANDROID_PROBE_FIXTURE_READY';
const fixtureTimeoutMs = 120_000;
const fixturePollMs = 100;
const maxDiagnosticBytes = 64 * 1024;
const serial = process.env.ANDROID_SERIAL;
const commandTimeoutMs = timeout(process.env.ANDROID_PROBE_COMMAND_TIMEOUT_MS, 15_000);
const instrumentationTimeoutMs = timeout(process.env.ANDROID_PROBE_INSTRUMENTATION_TIMEOUT_MS, 300_000);

function timeout(value, fallback) {
    const parsed = value === undefined ? fallback : Number(value);
    if (!Number.isSafeInteger(parsed) || parsed <= 0) throw new Error('Invalid probe timeout');
    return parsed;
}

function adb(args, {timeoutMs = commandTimeoutMs, artifact} = {}) {
    const result = spawnSync('adb', ['-s', serial, ...args], {
        encoding: 'utf8', timeout: timeoutMs, killSignal: 'SIGKILL', maxBuffer: maxDiagnosticBytes,
    });
    const diagnostics = `${result.stdout ?? ''}${result.stderr ?? ''}`;
    if (artifact) fs.writeFileSync(path.join(output, artifact), diagnostics);
    if (result.status === 0 && !result.error) return result.stdout;

    // Save only this command's bounded output; never collect unrelated device logs.
    fs.appendFileSync(path.join(output, 'adb-failure.txt'),
        `${args.join(' ')}: ${result.error?.code ?? result.status ?? result.signal}\n${diagnostics}\n`);
    throw new Error(`adb ${args[0]} failed; see adb-failure.txt`);
}

function mappings() {
    return adb(['reverse', '--list']).trim().split('\n').map(line => line.trim().split(/\s+/));
}

// Verify an explicitly selected emulator before any device mutation.
fs.mkdirSync(output, {recursive: true});
if (!serial || !/^emulator-\d+$/.test(serial)) throw new Error('Select an emulator with ANDROID_SERIAL');
if (adb(['get-state']).trim() !== 'device' || adb(['shell', 'getprop', 'ro.kernel.qemu']).trim() !== '1') {
    throw new Error('Selected target is not an emulator');
}
if (adb(['shell', 'pm', 'list', 'packages', packageName]).includes('package:')) {
    throw new Error('Use a disposable emulator without Copywraith installed');
}
if (mappings().some(([, source]) => source === fixturePort)) {
    throw new Error('Fixture reverse port is already in use');
}
for (const apk of [appApk, testApk]) {
    if (!fs.existsSync(apk)) throw new Error(`Missing ${apk}; run build.sh`);
}

const fixtureLog = fs.openSync(path.join(output, 'fixture.log'), 'w');
const fixture = spawn('cargo', ['test', '--locked', '-p', 'copywraith-server', '--test', 'android_runtime', 'serve_android_probe', '--', '--ignored', '--exact', '--nocapture'], {
    detached: true,
    stdio: ['ignore', fixtureLog, fixtureLog],
});
let fixtureFailed = false;
fixture.on('error', () => {
    fixtureFailed = true;
    fs.writeSync(fixtureLog, 'Fixture process could not start\n');
});
const installed = [];
let ownsMapping = false;
try {
    const deadline = Date.now() + fixtureTimeoutMs;
    while (!fs.readFileSync(path.join(output, 'fixture.log'), 'utf8').includes(fixtureReady)) {
        if (fixtureFailed || fixture.exitCode !== null || Date.now() > deadline) throw new Error('Fixture did not become ready; see fixture.log');
        await new Promise(resolve => setTimeout(resolve, fixturePollMs));
    }
    adb(['install', appApk]);
    installed.push(packageName);
    adb(['install', testApk]);
    installed.push(`${packageName}.test`);
    adb(['shell', 'pm', 'grant', packageName, 'android.permission.POST_NOTIFICATIONS']);
    // Refuse a mapping created after preflight instead of replacing it.
    adb(['reverse', '--no-rebind', fixturePort, fixturePort]);
    ownsMapping = true;
    const result = adb(['shell', 'am', 'instrument', '-w', '-r', '-e', 'class', `${packageName}.RuntimeLifecycleTest`, `${packageName}.test/androidx.test.runner.AndroidJUnitRunner`], {
        timeoutMs: instrumentationTimeoutMs, artifact: 'instrumentation.txt',
    });
    if (!/OK \(1 test\)/.test(result) || /FAILURES|INSTRUMENTATION_FAILED/.test(result)) {
        throw new Error('Lifecycle gate failed; see instrumentation.txt');
    }
    console.log('Android lifecycle gate passed');
} finally {
    if (fixture.pid) {
        try { process.kill(-fixture.pid, 'SIGKILL'); } catch {}
    }
    fs.closeSync(fixtureLog);
    if (ownsMapping) {
        try {
            if (mappings().some(([, source, target]) => source === fixturePort && target === fixturePort)) {
                adb(['reverse', '--remove', fixturePort]);
            }
        } catch { /* Failure diagnostics are already saved; preserve unknown mappings. */ }
    }
    for (const name of installed.reverse()) {
        try { adb(['uninstall', name]); } catch { /* Retain the original test failure. */ }
    }
}
