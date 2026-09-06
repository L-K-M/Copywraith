import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const ANDROID_SOURCE = new URL('../../crates/copywraith-share-target/android/src/main/', import.meta.url);

// Supplement the parcel tests; this cannot prove Android lifecycle behavior.
test('privileged clipboard capture cannot independently upload to the server', async () => {
	const source = await readFile(new URL('java/ShizukuClipboardService.kt', ANDROID_SOURCE), 'utf8');
	const aidl = await readFile(new URL('aidl/ch/lkmc/copywraith/share/IShizukuClipboardService.aidl', ANDROID_SOURCE), 'utf8');

	assert.doesNotMatch(aidl, /apiKey|ServerUrl|callingPackage/,
		'the privileged process uses its own identity and receives no server configuration');
	assert.doesNotMatch(source, /java\.net\.|HttpURLConnection|\/api\/entries/,
		'the app-owned durable producer must freeze every sync operation');
	assert.doesNotMatch(source, /lastUploadedHash/,
		'an in-memory hash is neither a durable receipt nor re-copy identity');
});
