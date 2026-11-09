import * as vscode from 'vscode';
import {
    LanguageClient,
    LanguageClientOptions,
    ServerOptions,
    TransportKind
} from 'vscode-languageclient/node';
import { execFileText, resolveBinaryPath } from './binary';

let client: LanguageClient | undefined;

/**
 * Resolves the `hyperion-lsp` binary and launches it with the `dap`
 * subcommand (see `hyperion-lsp/src/main.rs`'s `Commands::Dap` arm, which
 * runs `hyperion_dap::server::run` over stdio) whenever a `hyperion`-type
 * debug session is started.
 *
 * Reuses the same binary resolution as the language client so debugging
 * respects `hyperion.serverPath` and falls back to `PATH`/bundled binary
 * the same way.
 */
class HyperionDebugAdapterDescriptorFactory implements vscode.DebugAdapterDescriptorFactory {
    constructor(
        private readonly outputChannel: vscode.OutputChannel
    ) {}

    async createDebugAdapterDescriptor(
        _session: vscode.DebugSession,
        _executable: vscode.DebugAdapterExecutable | undefined
    ): Promise<vscode.DebugAdapterDescriptor> {
        const config = vscode.workspace.getConfiguration('hyperion');
        const serverPath = await resolveBinaryPath(config.get<string>('serverPath'));

        if (!serverPath) {
            const message =
                'Hyperion debugger: hyperion-lsp binary not found. Build it from source, ' +
                'put it on PATH, or configure hyperion.serverPath.';
            this.outputChannel.appendLine(`ERROR: ${message}`);
            throw new Error(message);
        }

        this.outputChannel.appendLine(`Starting Hyperion DAP server: "${serverPath}" dap`);
        return new vscode.DebugAdapterExecutable(serverPath, ['dap']);
    }
}

export async function activate(context: vscode.ExtensionContext) {
    const outputChannel = vscode.window.createOutputChannel('Hyperion LSP');
    outputChannel.appendLine('Hyperion LSP extension activating...');

    context.subscriptions.push(
        vscode.debug.registerDebugAdapterDescriptorFactory(
            'hyperion',
            new HyperionDebugAdapterDescriptorFactory(outputChannel)
        )
    );

    const config = vscode.workspace.getConfiguration('hyperion');
    const serverPath = await resolveBinaryPath(config.get<string>('serverPath'));

    if (!serverPath) {
        const message = 'Hyperion LSP binary not found. Build it from source, put it on PATH, or configure hyperion.serverPath.';
        outputChannel.appendLine(`ERROR: ${message}`);
        vscode.window.showErrorMessage(message);
        return;
    }

    outputChannel.appendLine(`Using server path: ${serverPath}`);

    // Check binary version compatibility
    try {
        const versionOutput = await execFileText(serverPath, ['--version']);
        const versionMatch = versionOutput.trim().match(/\d+\.\d+\.\d+/);
        if (versionMatch) {
            const version = versionMatch[0];
            const minVersion = '0.1.0';

            // Simple semver comparison (major.minor.patch)
            const [vMajor, vMinor, vPatch] = version.split('.').map(Number);
            const [minMajor, minMinor, minPatch] = minVersion.split('.').map(Number);

            const isCompatible = vMajor > minMajor ||
                                (vMajor === minMajor && vMinor > minMinor) ||
                                (vMajor === minMajor && vMinor === minMinor && vPatch >= minPatch);

            if (!isCompatible) {
                const message = `Hyperion LSP version ${version} is older than required ${minVersion}. Please update.`;
                outputChannel.appendLine(`WARNING: ${message}`);
                vscode.window.showWarningMessage(message);
            } else {
                outputChannel.appendLine(`Binary version: ${version} (compatible)`);
            }
        }
    } catch (error) {
        outputChannel.appendLine(`Warning: Could not check binary version: ${error}`);
    }

    const serverOptions: ServerOptions = {
        run: {
            command: serverPath,
            transport: TransportKind.stdio
        },
        debug: {
            command: serverPath,
            transport: TransportKind.stdio
        }
    };

    const clientOptions: LanguageClientOptions = {
        documentSelector: [
            { scheme: 'file', language: 'java' },
            { scheme: 'file', language: 'kotlin' }
        ],
        synchronize: {
            fileEvents: vscode.workspace.createFileSystemWatcher('**/*.{java,kt}')
        },
        outputChannel: outputChannel
    };

    client = new LanguageClient(
        'hyperion-lsp',
        'Hyperion LSP',
        serverOptions,
        clientOptions
    );

    try {
        outputChannel.appendLine('Starting language client...');
        await client.start();
        outputChannel.appendLine('Language client started successfully!');
        context.subscriptions.push(client);
    } catch (error) {
        outputChannel.appendLine(`Failed to start language client: ${error}`);
        vscode.window.showErrorMessage(`Hyperion LSP failed to start: ${error}`);
    }
}

export async function deactivate(): Promise<void> {
    if (client) {
        await client.stop();
    }
}
