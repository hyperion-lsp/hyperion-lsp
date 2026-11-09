// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

import * as fs from 'fs';
import { execFile } from 'child_process';

export type BinaryResolverDependencies = {
    isExecutable: (filePath: string) => boolean;
    findBinaryInPath: () => Promise<string | null>;
};

/** Run an executable directly, without shell parsing. */
export function execFileText(file: string, args: readonly string[]): Promise<string> {
    return new Promise((resolve, reject) => {
        execFile(file, args, {
            encoding: 'utf8',
            maxBuffer: 64 * 1024,
            timeout: 5_000,
            windowsHide: true
        }, (error, stdout) => {
            if (error) {
                reject(error);
            } else {
                resolve(stdout);
            }
        });
    });
}

/** Check whether a candidate is an executable file on the current platform. */
export function isExecutable(
    filePath: string,
    platform: NodeJS.Platform = process.platform
): boolean {
    try {
        if (!fs.statSync(filePath).isFile()) {
            return false;
        }
        if (platform === 'win32') {
            return filePath.toLowerCase().endsWith('.exe');
        }
        fs.accessSync(filePath, fs.constants.X_OK);
        return true;
    } catch {
        return false;
    }
}

/** Return the first non-empty path emitted by `which` or Windows `where`. */
export function firstBinaryPath(stdout: string): string | null {
    return stdout
        .split(/\r?\n/)
        .map(line => line.trim())
        .find(line => line.length > 0) ?? null;
}

/** Find `hyperion-lsp` on PATH without invoking a command shell. */
export async function findBinaryInPath(
    platform: NodeJS.Platform = process.platform,
    run: (file: string, args: readonly string[]) => Promise<string> = execFileText
): Promise<string | null> {
    try {
        const locator = platform === 'win32' ? 'where' : 'which';
        return firstBinaryPath(await run(locator, ['hyperion-lsp']));
    } catch {
        return null;
    }
}

/** Resolve a configured binary first, then fall back to PATH. */
export async function resolveBinaryPath(
    configuredPath: string | undefined,
    dependencies: BinaryResolverDependencies = {
        isExecutable,
        findBinaryInPath
    }
): Promise<string | null> {
    if (configuredPath && dependencies.isExecutable(configuredPath)) {
        return configuredPath;
    }

    const pathBinary = await dependencies.findBinaryInPath();
    if (pathBinary && dependencies.isExecutable(pathBinary)) {
        return pathBinary;
    }

    return null;
}
