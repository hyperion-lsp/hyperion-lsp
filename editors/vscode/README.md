# Hyperion LSP for VS Code

This development extension starts the `hyperion-lsp` language server for Java
and Kotlin and registers its Java debug adapter. It is not published on the VS
Code Marketplace and does not bundle the server binary.

Build the server from the repository root:

```sh
cargo build --release -p hyperion-lsp
```

Install the extension dependencies and compile it:

```sh
cd editors/vscode
npm install
npm run compile
```

Put `target/release/hyperion-lsp` on `PATH`, or set
`hyperion.serverPath` to its absolute path. Run the extension from VS Code's
Extension Development Host, or package a local VSIX with
`npx @vscode/vsce package`.

See the [project README](https://github.com/hyperion-lsp/hyperion-lsp) for the
supported feature matrix, known rough edges, and licensing terms.
