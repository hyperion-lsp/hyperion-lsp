-- Copyright 2026 Adrian Helvik
-- SPDX-License-Identifier: PolyForm-Shield-1.0.0

--- Lookup and request helpers for the Hyperion client.

local config = require('hyperion.config')

local M = {}

---@param msg string
---@param level? integer
function M.notify(msg, level)
  vim.notify('Hyperion: ' .. msg, level or vim.log.levels.INFO)
end

--- The client attached to `bufnr`, or any running Hyperion client when `workspace` is true.
---@param bufnr? integer
---@param workspace? boolean
---@return vim.lsp.Client?
function M.get(bufnr, workspace)
  local client = vim.lsp.get_clients({ name = config.name, bufnr = bufnr or 0 })[1]
  if not client and workspace then
    client = vim.lsp.get_clients({ name = config.name })[1]
  end
  return client
end

--- Like `get()`, but tells the user why there is no client.
---@param bufnr? integer
---@param workspace? boolean
---@return vim.lsp.Client?
function M.require(bufnr, workspace)
  local client = M.get(bufnr, workspace)
  if not client then
    M.notify(
      'no language server is attached to this buffer. Open a Java, Kotlin or Groovy file in a '
        .. 'project, or enable it with vim.lsp.enable("hyperion").',
      vim.log.levels.WARN
    )
  end
  return client
end

--- Run a `workspace/executeCommand` the server advertises and pass its result to `on_result`.
---@param client vim.lsp.Client
---@param command string
---@param arguments? table[]
---@param bufnr? integer
---@param on_result fun(result: any)
function M.exec(client, command, arguments, bufnr, on_result)
  client:exec_cmd({ title = command, command = command, arguments = arguments }, {
    bufnr = bufnr,
  }, function(err, result)
    if err then
      M.notify(command .. ' failed: ' .. (err.message or vim.inspect(err)), vim.log.levels.ERROR)
      return
    end
    on_result(result)
  end)
end

return M
