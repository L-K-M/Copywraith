import fs from 'node:fs';
import path from 'node:path';

const root = path.resolve('src-tauri/gen/android/app');
const fixtures = path.resolve('scripts/android-runtime-probe');
if (!fs.existsSync(root)) throw new Error('Run tauri android init --skip-targets-install first');
for (const sourceSet of ['debug', 'androidTest']) {
    const destination = path.join(root, 'src', sourceSet);
    fs.mkdirSync(path.join(destination, 'java/ch/lkmc/copywraith'), {recursive: true});
    for (const file of fs.readdirSync(path.join(fixtures, sourceSet))) {
        const target = file.endsWith('.kt') ? path.join(destination, 'java/ch/lkmc/copywraith', file) : path.join(destination, file);
        fs.copyFileSync(path.join(fixtures, sourceSet, file), target);
    }
}
const gradle = path.join(root, 'build.gradle.kts');
let source = fs.readFileSync(gradle, 'utf8');
const runner = 'testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"';
if (!source.includes(runner)) source = source.replace('defaultConfig {', `defaultConfig {\n        ${runner}`);
fs.writeFileSync(gradle, source);
