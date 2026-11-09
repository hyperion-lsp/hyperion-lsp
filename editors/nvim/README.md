# hyperion.nvim

Neovim client for [Hyperion](https://github.com/hyperion-lsp/hyperion-lsp), a
Java, Kotlin and Groovy language server written in Rust. It is a thin layer over
Neovim's built-in LSP client:

- an `lsp/hyperion.lua` config, so `vim.lsp.enable('hyperion')` works with no setup;
- a `:Hyperion` command: reload the project model, run a configuration, debug,
  run the code lens under the cursor, read the server output;
- an optional [nvim-dap](https://github.com/mfussenegger/nvim-dap) adapter and
  configurations for debugging Java and Kotlin `main` methods;
- `:checkhealth hyperion`.

It needs **Neovim 0.11 or newer** and has no dependencies. nvim-dap is optional.

This directory is the plugin's source of truth. It is published on its own as
`hyperion-lsp/hyperion.nvim`; report issues on the main repository.

## Install

The plugin does not include the server. Build `hyperion-lsp` from source (see
[Try it](https://github.com/hyperion-lsp/hyperion-lsp#try-it)) and put it on your
`$PATH`, or point `server_path` at it. There is no packaged distribution yet.

### lazy.nvim

```lua
{
  'hyperion-lsp/hyperion.nvim',
  opts = {}, -- calls require('hyperion').setup(opts), which enables the server
}
```

Do not lazy-load it by filetype; it has to be on the runtimepath before the first
buffer attaches.

### vim.pack (Neovim 0.12)

```lua
vim.pack.add({ 'https://github.com/hyperion-lsp/hyperion.nvim' })

vim.lsp.enable('hyperion') -- or: require('hyperion').setup({ ... })
```

### Manual

Clone it into a native package directory:

```sh
git clone https://github.com/hyperion-lsp/hyperion.nvim \
  ~/.local/share/nvim/site/pack/hyperion/start/hyperion.nvim
```

or, from a checkout of the Hyperion repository, add this directory to the
runtimepath:

```lua
vim.opt.runtimepath:append('/path/to/hyperion-lsp/editors/nvim')
vim.lsp.enable('hyperion')
```

### No plugin

The plugin only adds convenience. The server works with Neovim alone:

```lua
vim.lsp.config('hyperion', {
  cmd = { '/path/to/hyperion-lsp' },
  filetypes = { 'java', 'kotlin', 'groovy' },
  root_markers = {
    { 'settings.gradle', 'settings.gradle.kts' },
    '.git',
    { 'pom.xml', 'build.gradle', 'build.gradle.kts' },
  },
})
vim.lsp.enable('hyperion')
```

## Configuration

`setup()` is optional. These are the defaults:

```lua
require('hyperion').setup({
  server_path = nil,          -- path to hyperion-lsp; $PATH is searched when nil
  auto_enable = true,         -- call vim.lsp.enable('hyperion')
  extra_filetypes = {},       -- added to java, kotlin, groovy; e.g. { 'scala' }
  init_options = nil,         -- LSP initializationOptions
  settings = nil,             -- sent with workspace/didChangeConfiguration
  build_tool_classpath = nil, -- true: let the server run Gradle/Maven (see below)
  codelens = true,            -- show and refresh code lenses ("Run" above main methods)
  dap = true,                 -- register the nvim-dap adapter when nvim-dap is installed
})
```

Anything else goes through Neovim's own API, which `setup()` merges with:

```lua
vim.lsp.config('hyperion', {
  cmd_env = { HYPERION_DEBUG = '1' }, -- verbose server log
  on_attach = function(client, bufnr) end,
})
```

Scala is thin (hover and go-to-definition by name; see the main README) and so
off by default. Add it with `extra_filetypes = { 'scala' }`.

Neovim picks the project root from the nearest `settings.gradle(.kts)`, else the
nearest `.git`, else the nearest `pom.xml` or `build.gradle(.kts)`, so files in a
Gradle submodule attach to the top-level build.

### Letting Hyperion run your build tool

By default Hyperion reads Gradle and Maven projects without executing them and
resolves dependencies from the local caches. Setting `build_tool_classpath = true`
lets it run `./gradlew` or `mvn` to get exact classpaths. **That executes the
project's build scripts and plugins, which is arbitrary code from the
repository.** It is a deliberate opt-in; enable it for projects you trust. It is
sent as `{ hyperion = { buildToolClasspath = true } }` in both
`initializationOptions` and `settings`.

## Commands

| Command | |
|---|---|
| `:Hyperion reload` | Re-read the Gradle/Maven model (`hyperion.reloadProject`). |
| `:Hyperion run [name]` | Choose an application run configuration from `.run/*.xml` or `.idea/runConfigurations` and run its `main` in the server's bytecode VM (`hyperion.run`). |
| `:Hyperion debug` | Build the `main` method under the cursor and debug it with nvim-dap. `:Hyperion launch` is the same. |
| `:Hyperion codelens` | Run the code lens on the cursor line (the "Run" lens above `main`). |
| `:Hyperion output` | Open the server's `window/logMessage` output; the stdout and stderr of a run end up here. |
| `:Hyperion log` | Open Neovim's LSP log. |
| `:Hyperion info` | Show the server binary, version, client, root and nvim-dap status. |

When no server is attached the commands say so instead of failing. Nothing is
bound to a key; for example:

```lua
vim.keymap.set('n', '<leader>cr', '<Cmd>Hyperion codelens<CR>', { desc = 'Hyperion: run main' })
```

`hyperion.run` is executed by the server itself, in its bytecode VM, without
starting a JVM process. It suits quick `main` methods; `:Hyperion debug` starts a
real JVM. Hyperion has no test runner, so `hyperion.runTest` only reports that it
is unsupported, and `hyperion.showImplementations` (which the server no longer
emits) is mapped to `vim.lsp.buf.implementation()`.

## Debugging

With [nvim-dap](https://github.com/mfussenegger/nvim-dap) installed the plugin
registers `dap.adapters.hyperion` (it runs `hyperion-lsp dap`) and, for `java` and
`kotlin`, two configurations that `:DapContinue` offers:

- **launch the main method in this file** asks the server how to launch the
  method under the cursor (`hyperion.resolveLaunch`), runs the build step it
  returns (`javac`, `gradle` or `mvn`; a failure stops the session with the
  compiler output), and starts the program under the debugger with the classpath
  the server resolved. `:Hyperion debug` starts it directly.
- **attach to a JVM (JDWP)** prompts for a port (default 5005) and attaches to
  `127.0.0.1`, for a JVM started with `-agentlib:jdwp=transport=dt_socket,server=y,suspend=y,address=5005`.

The launch configuration accepts the adapter's fields if you want to set them
yourself; a configuration with `mainClass` is passed through untouched:

```lua
require('dap').configurations.java = {
  {
    type = 'hyperion',
    request = 'launch',
    name = 'Main with arguments',
    mainClass = 'com.example.Main',
    classpath = vim.fn.getcwd() .. '/build/classes/java/main',
    args = { '--verbose' },
    jvmArgs = { '-Xmx512m' },
    cwd = vim.fn.getcwd(),
    projectRoot = vim.fn.getcwd(),
    env = { FOO = 'bar' },
  },
}
```

Limits: only `main` methods are launched; debugging a test (which the server
describes as a build-tool command plus an attach) is not wired up. Set
`dap = false` to leave nvim-dap alone, or define `dap.adapters.hyperion` yourself
to replace the adapter.

## Troubleshooting

Start with `:checkhealth hyperion` and `:Hyperion info`.

- **Nothing attaches.** The server must be on `$PATH` or in `server_path`, and the
  file's `filetype` must be `java`, `kotlin` or `groovy`. When the binary is missing
  `setup()` warns and does not enable the server; Neovim itself stays quiet about an
  `lsp/` config whose command is not executable. Check `vim.lsp.is_enabled('hyperion')`.
- **Wrong root, so each submodule starts its own server.** `:Hyperion info` prints the
  root. Override `root_markers` or `root_dir` with `vim.lsp.config('hyperion', ...)`.
- **Missing dependencies, unresolved library symbols.** Build the project once so the
  caches are filled, or see `build_tool_classpath` above.
- **The Run lens does nothing visible.** The result is a short message; the program's
  output is in `:Hyperion output`. Code lenses are implemented for Java, not Kotlin.
- **The server log.** `:Hyperion log` is Neovim's side (set
  `vim.lsp.log.set_level('debug')` for traffic). The server writes its own log to
  `$TMPDIR/hyperion-lsp-<pid>.log`; `HYPERION_DEBUG=1` adds detail.

## License

[PolyForm Shield 1.0.0](LICENSE): free for any use except building a competing
product. See the main repository for the exact terms.

## Development

The plugin lives in [`editors/nvim`](https://github.com/hyperion-lsp/hyperion-lsp/tree/main/editors/nvim)
of the main Hyperion repository; please open issues and pull requests there.
[hyperion-lsp/hyperion.nvim](https://github.com/hyperion-lsp/hyperion.nvim) is a read-only mirror
that a scheduled workflow updates from that directory.
