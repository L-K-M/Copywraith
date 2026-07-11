import { readFileSync } from 'node:fs';

const expectedVersion = process.argv[2];
if (!/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(expectedVersion ?? '')) {
	console.error(`Expected a semantic version, received: ${expectedVersion ?? '<missing>'}`);
	process.exit(1);
}

const jsonManifests = [
	['package.json', (json) => json.version],
	['package-lock.json', (json) => json.packages?.['']?.version],
	['server/ui/package.json', (json) => json.version],
	['server/ui/package-lock.json', (json) => json.packages?.['']?.version],
	['src-tauri/tauri.conf.json', (json) => json.version]
];

const cargoManifests = [
	'crates/copywraith-core/Cargo.toml',
	'crates/copywraith-share-target/Cargo.toml',
	'server/Cargo.toml',
	'src-tauri/Cargo.toml'
];

const versions = [];
for (const [path, getVersion] of jsonManifests) {
	const json = JSON.parse(readFileSync(path, 'utf8'));
	versions.push([path, getVersion(json)]);
}

for (const path of cargoManifests) {
	const manifest = readFileSync(path, 'utf8');
	const packageStart = manifest.indexOf('[package]');
	const afterPackage = packageStart < 0 ? '' : manifest.slice(packageStart + '[package]'.length);
	const nextSection = afterPackage.search(/^\[/m);
	const packageSection = nextSection < 0 ? afterPackage : afterPackage.slice(0, nextSection);
	const version = packageSection.match(/^version\s*=\s*"([^"]+)"\s*$/m)?.[1];
	versions.push([path, version]);
}

const mismatches = versions.filter(([, version]) => version !== expectedVersion);
if (mismatches.length > 0) {
	console.error(`Release tag version ${expectedVersion} does not match:`);
	for (const [path, version] of mismatches) {
		console.error(`- ${path}: ${version ?? '<missing>'}`);
	}
	process.exit(1);
}

console.log(`All ${versions.length} manifests match ${expectedVersion}.`);
