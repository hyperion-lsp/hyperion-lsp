-- Copyright 2026 Adrian Helvik
-- SPDX-License-Identifier: PolyForm-Shield-1.0.0

--- Defaults and resolved settings shared by the plugin, `lsp/hyperion.lua` and `:checkhealth`.

local M = {}

--- Name of the `vim.lsp.config` entry (and of the client).
M.name = 'hyperion'

--- Default executable, looked up on `$PATH`.
M.binary = 'hyperion-lsp'

--- Scala is opt-in through `extra_filetypes` (see the README for its limits).
M.filetypes = { 'java', 'kotlin', 'groovy' }

--- Earlier entries win over later ones; names in one inner list have equal priority. A Gradle
--- `settings` file marks the top of a multi-module build, so it beats the nearest `build.gradle`.
M.root_markers = {
  { 'settings.gradle', 'settings.gradle.kts' },
  '.git',
  { 'pom.xml', 'build.gradle', 'build.gradle.kts' },
}

---@class hyperion.Options
---@field server_path? string Path to `hyperion-lsp`; `$PATH` is searched when unset.
---@field auto_enable boolean Call `vim.lsp.enable('hyperion')` from `setup()`.
---@field extra_filetypes string[] Added to `java`, `kotlin` and `groovy`, e.g. `{ 'scala' }`.
---@field init_options? table Sent as LSP `initializationOptions`.
---@field settings? table Sent as `workspace/didChangeConfiguration` settings.
---@field build_tool_classpath? boolean Let the server run Gradle/Maven to resolve classpaths.
---@field codelens boolean Show and refresh code lenses (the "Run" lens) in attached buffers.
---@field dap boolean Register the nvim-dap adapter and configurations when nvim-dap is installed.
M.defaults = {
  server_path = nil,
  auto_enable = true,
  extra_filetypes = {},
  init_options = nil,
  settings = nil,
  build_tool_classpath = nil,
  codelens = true,
  dap = true,
}

---@type hyperion.Options
M.options = vim.deepcopy(M.defaults)

--- The command the client is started with, after `setup()` and any user `vim.lsp.config()`.
---@return string[]|function
function M.cmd()
  local config = vim.lsp.config[M.name]
  return config and config.cmd or { M.binary }
end

--- The server executable, or nil when `cmd` is a function (a custom launcher).
---@return string?
function M.server()
  local cmd = M.cmd()
  return type(cmd) == 'table' and cmd[1] or nil
end

--- Whether `vim.lsp.enable('hyperion')` is in effect; nil when this Neovim cannot tell.
---@return boolean?
function M.enabled()
  if vim.lsp.is_enabled then
    return vim.lsp.is_enabled(M.name)
  end
end

return M
