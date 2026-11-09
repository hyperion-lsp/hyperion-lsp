// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

const assert = require('node:assert/strict');
const { test } = require('node:test');
const manifest = require('../package.json');
const lock = require('../package-lock.json');

test('editor minimum and API types match the locked language client requirement', () => {
    const clientEngine = lock.packages['node_modules/vscode-languageclient'].engines.vscode;
    assert.equal(manifest.engines.vscode, clientEngine);
    assert.equal(manifest.devDependencies['@types/vscode'], clientEngine.slice(1));
    assert.equal(lock.packages['node_modules/@types/vscode'].version, clientEngine.slice(1));
    assert.deepEqual(lock.packages[''].engines, manifest.engines);
    assert.deepEqual(lock.packages[''].devDependencies, manifest.devDependencies);
});

test('Kotlin files have a contributed language matching activation', () => {
    const kotlin = manifest.contributes.languages.find(language => language.id === 'kotlin');
    assert.ok(kotlin, 'clean editor profiles need a Kotlin language association');
    assert.ok(kotlin.extensions.includes('.kt'));
    assert.ok(kotlin.extensions.includes('.kts'));
    assert.ok(manifest.activationEvents.includes(`onLanguage:${kotlin.id}`));
});
