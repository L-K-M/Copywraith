import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const ANDROID_SOURCE = new URL('../../crates/copywraith-share-target/android/src/main/', import.meta.url);

// These boundary checks supplement, not replace, Android lifecycle tests.
test('background capture has a private foreground service and notification permission', async () => {
	const manifest = await readFile(new URL('AndroidManifest.xml', ANDROID_SOURCE), 'utf8');
	const services = manifest.match(/<service\b[^>]*>/g) ?? [];

	assert.ok(services.some(service =>
		/android:foregroundServiceType=/.test(service) && /android:exported="false"/.test(service)),
	'background capture must not depend on an Activity');
	assert.match(manifest, /android\.permission\.FOREGROUND_SERVICE"/);
	assert.match(manifest, /android\.permission\.POST_NOTIFICATIONS"/);
});
