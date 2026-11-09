-- Copyright 2026 Adrian Helvik
-- SPDX-License-Identifier: PolyForm-Shield-1.0.0

--- A scratch buffer holding the server's `window/logMessage` output.
---
--- Neovim writes these messages to its LSP log, which drops everything below WARN by default.
--- The "Run" code lens prints the program's stdout and stderr this way, so they are kept here.

local M = {}

local MAX_LINES = 5000
local LABELS = { 'error', 'warn', 'info', 'log' } -- LSP MessageType 1..4
local buf ---@type integer?

local function get_buf()
  if buf and vim.api.nvim_buf_is_valid(buf) then
    return buf
  end
  buf = vim.api.nvim_create_buf(false, true)
  vim.api.nvim_buf_set_name(buf, 'hyperion://output')
  vim.bo[buf].bufhidden = 'hide'
  vim.bo[buf].modifiable = false
  return buf
end

---@param message string
---@param type? integer LSP `MessageType`; defaults to Log
function M.append(message, type)
  type = type or 4
  local b = get_buf()
  local lines = vim.split(message, '\n', { plain = true })
  if type <= 2 then
    lines[1] = ('[%s] %s'):format(LABELS[type], lines[1])
  end
  vim.bo[b].modifiable = true
  local count = vim.api.nvim_buf_line_count(b)
  local fresh = count == 1 and vim.api.nvim_buf_get_lines(b, 0, 1, false)[1] == ''
  vim.api.nvim_buf_set_lines(b, fresh and 0 or count, -1, false, lines)
  count = vim.api.nvim_buf_line_count(b)
  if count > MAX_LINES then
    vim.api.nvim_buf_set_lines(b, 0, count - MAX_LINES, false, {})
  end
  vim.bo[b].modifiable = false
  for _, win in ipairs(vim.fn.win_findbuf(b)) do
    vim.api.nvim_win_set_cursor(win, { vim.api.nvim_buf_line_count(b), 0 })
  end
end

--- `window/logMessage` handler: record the message, then defer to Neovim's own handler.
---@type lsp.Handler
function M.handler(err, result, ctx)
  if result and type(result.message) == 'string' then
    M.append(result.message, result.type)
  end
  return vim.lsp.handlers['window/logMessage'](err, result, ctx)
end

--- Show the buffer in a split at the bottom of the screen.
function M.open()
  local b = get_buf()
  if #vim.fn.win_findbuf(b) == 0 then
    vim.cmd('botright 12split')
    vim.api.nvim_win_set_buf(0, b)
  end
  vim.api.nvim_win_set_cursor(vim.fn.win_findbuf(b)[1], { vim.api.nvim_buf_line_count(b), 0 })
end

return M
