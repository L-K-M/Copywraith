import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import ts from 'typescript';

// Both frontends must enforce the same rule as copywraith-core's
// bearer_password_problem, or a password could pass one check and fail another.
function load(path) {
	const source = readFileSync(new URL(path, import.meta.url), 'utf8');
	const module = { exports: {} };
	new Function('module', 'exports', ts.transpileModule(source, {
		compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS }
	}).outputText)(module, module.exports);
	return module.exports.bearerPasswordProblem;
}

const popup = load('../../src/lib/util/password.ts');
const admin = load('../../server/ui/src/lib/password.ts');

for (const password of ['correct horse battery', 'p@ss~word!123']) {
	test(`${JSON.stringify(password)} is usable in both frontends`, () => {
		assert.equal(popup(password), null);
		assert.equal(admin(password), null);
	});
}

for (const password of ['Grüezi-2026', 'password123 ', ' password123', 'tab\tinside']) {
	test(`${JSON.stringify(password)} is rejected with the same message in both frontends`, () => {
		assert.ok(popup(password));
		assert.equal(popup(password), admin(password));
	});
}
