// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

import { strict as assert } from 'node:assert';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { test } from 'node:test';
import {
    findBinaryInPath,
    firstBinaryPath,
    isExecutable,
    resolveBinaryPath
} from './binary';

test('firstBinaryPath handles Windows output and blank lines', () => {
    assert.equal(
        firstBinaryPath('\r\nC:\\Tools\\hyperion-lsp.exe\r\nC:\\Other\\hyperion-lsp.exe\r\n'),
        'C:\\Tools\\hyperion-lsp.exe'
    );
    assert.equal(firstBinaryPath('\n\r\n'), null);
});

test('isExecutable accepts files and rejects directories', () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'hyperion-vscode-'));
    try {
        const executable = path.join(root, 'hyperion-lsp');
        fs.writeFileSync(executable, 'fixture');
        fs.chmodSync(executable, 0o755);

        assert.equal(isExecutable(executable, 'linux'), true);
        fs.chmodSync(executable, 0o644);
        assert.equal(isExecutable(executable, 'linux'), false);
        assert.equal(isExecutable(root, 'linux'), false);
        assert.equal(isExecutable(path.join(root, 'missing'), 'linux'), false);
    } finally {
        fs.rmSync(root, { recursive: true });
    }
});

test('findBinaryInPath uses argv spawning rather than a shell command', async () => {
    const calls: Array<{ file: string; args: readonly string[] }> = [];
    const result = await findBinaryInPath('linux', async (file, args) => {
        calls.push({ file, args });
        return '/opt/hyperion/bin/hyperion-lsp\n';
    });

    assert.equal(result, '/opt/hyperion/bin/hyperion-lsp');
    assert.deepEqual(calls, [{ file: 'which', args: ['hyperion-lsp'] }]);
});

test('configured executable wins without consulting PATH', async () => {
    let pathConsulted = false;
    const result = await resolveBinaryPath('/configured/hyperion-lsp', {
        isExecutable: candidate => candidate === '/configured/hyperion-lsp',
        findBinaryInPath: async () => {
            pathConsulted = true;
            return '/path/hyperion-lsp';
        }
    });

    assert.equal(result, '/configured/hyperion-lsp');
    assert.equal(pathConsulted, false);
});

test('invalid configured path falls back to an executable on PATH', async () => {
    const result = await resolveBinaryPath('/missing/hyperion-lsp', {
        isExecutable: candidate => candidate === '/path/hyperion-lsp',
        findBinaryInPath: async () => '/path/hyperion-lsp'
    });

    assert.equal(result, '/path/hyperion-lsp');
});

test('resolver declines missing and non-executable candidates', async () => {
    const result = await resolveBinaryPath(undefined, {
        isExecutable: () => false,
        findBinaryInPath: async () => '/path/non-executable-hyperion-lsp'
    });

    assert.equal(result, null);
});
