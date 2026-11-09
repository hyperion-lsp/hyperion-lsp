-- Copyright 2026 Adrian Helvik
-- SPDX-License-Identifier: PolyForm-Shield-1.0.0

--- Neovim 0.11+ LSP config. `vim.lsp.enable('hyperion')` is all that is needed once the plugin is
--- on the runtimepath; `require('hyperion').setup()` only layers user options on top.
local config = require('hyperion.config')

---@type vim.lsp.Config
return {
  cmd = { config.binary },
  filetypes = config.filetypes,
  root_markers = config.root_markers,
  handlers = {
    ['window/logMessage'] = require('hyperion.output').handler,
  },
  -- Commands the server still accepts from old clients but no longer advertises. Handling them
  -- here keeps a cached code lens from ending in "language server does not support command".
  commands = {
    ['hyperion.showImplementations'] = function()
      vim.lsp.buf.implementation()
    end,
    ['hyperion.runTest'] = function()
      vim.notify(
        'Hyperion: running tests is not supported; use your build tool (Gradle or Maven).',
        vim.log.levels.WARN
      )
    end,
  },
}
