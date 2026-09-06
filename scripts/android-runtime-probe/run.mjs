import fs from 'node:fs';
import path from 'node:path';
import {spawn, spawnSync} from 'node:child_process';

const packageName = 'ch.lkmc.copywraith';
const fixturePort = 18763;
const output = path.resolve('artifacts/android-runtime');
const apkRoot = 'src-tauri/gen/android/app/build/outputs/apk';
const appApk = `${apkRoot}/universal/debug/app-universal-debug.apk`;
const testApk = `${apkRoot}/androidTest/universal/debug/app-universal-debug-androidTest.apk`;
const fixtureReady = 'ANDROID_PROBE_FIXTURE_READY';
const timeoutMs = 120_000;

function adb(...args) {
    const result = spawnSync('adb', args, {encoding: 'utf8'});
    if (result.status !== 0) throw new Error(`adb ${args[0]} failed`);
    return result.stdout;
}

// A fresh disposable installation is required; never clear an existing user's app.
if (adb('shell', 'pm', 'list', 'packages', packageName).includes('package:')) {
    throw new Error('Use a disposable device without Copywraith installed');
}
for (const apk of [appApk, testApk]) {
    if (!fs.existsSync(apk)) throw new Error(`Missing ${apk}; run build.sh`);
}
fs.mkdirSync(output, {recursive: true});
const fixtureLog = fs.openSync(path.join(output, 'fixture.log'), 'w');
const fixture = spawn('cargo', ['test', '--locked', '-p', 'copywraith-server', '--test', 'android_runtime', 'serve_android_probe', '--', '--ignored', '--exact', '--nocapture'], {
    detached: true,
    stdio: ['ignore', fixtureLog, fixtureLog],
});
let installed = false;
try {
    const deadline = Date.now() + timeoutMs;
    while (!fs.readFileSync(path.join(output, 'fixture.log'), 'utf8').includes(fixtureReady)) {
        if (fixture.exitCode !== null || Date.now() > deadline) throw new Error('Fixture did not become ready');
        await new Promise(resolve => setTimeout(resolve, 100));
    }
    adb('install', appApk);
    installed = true;
    adb('install', testApk);
    adb('shell', 'pm', 'grant', packageName, 'android.permission.POST_NOTIFICATIONS');
    adb('reverse', `tcp:${fixturePort}`, `tcp:${fixturePort}`);
    const result = adb('shell', 'am', 'instrument', '-w', '-r', '-e', 'class', `${packageName}.RuntimeLifecycleTest`, `${packageName}.test/androidx.test.runner.AndroidJUnitRunner`);
    fs.writeFileSync(path.join(output, 'instrumentation.txt'), result);
    if (!/OK \(1 test\)/.test(result) || /FAILURES|INSTRUMENTATION_FAILED/.test(result)) {
        throw new Error('Lifecycle gate failed; see instrumentation.txt');
    }
    console.log('Android lifecycle gate passed');
} finally {
    try { process.kill(-fixture.pid, 'SIGTERM'); } catch {}
    fs.closeSync(fixtureLog);
    spawnSync('adb', ['reverse', '--remove', `tcp:${fixturePort}`], {stdio: 'ignore'});
    if (installed) {
        spawnSync('adb', ['uninstall', `${packageName}.test`], {stdio: 'ignore'});
        spawnSync('adb', ['uninstall', packageName], {stdio: 'ignore'});
    }
}
