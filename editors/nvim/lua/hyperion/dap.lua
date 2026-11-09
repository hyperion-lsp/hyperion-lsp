-- Copyright 2026 Adrian Helvik
-- SPDX-License-Identifier: PolyForm-Shield-1.0.0

--- Soft nvim-dap integration: nothing here runs unless nvim-dap can be required.
---
--- The server describes how to launch (`hyperion.resolveLaunch`); this module builds the project if
--- needed and hands the `launch` object to the `hyperion-lsp dap` adapter unchanged.

local client = require('hyperion.client')
local config = require('hyperion.config')

local M = {}

local PREFIX = 'Hyperion: '
local ATTACH_PORT = 5005

---@return table? dap
local function load()
  local ok, dap = pcall(require, 'dap')
  return ok and dap or nil
end

function M.available()
  return load() ~= nil
end

---@param on_done fun(ok: boolean)
local function build(spec, on_done)
  local cmd = spec.build
  if type(cmd) ~= 'table' or type(cmd.argv) ~= 'table' then
    return on_done(true)
  end
  local line = table.concat(cmd.argv, ' ')
  client.notify('building: ' .. line)
  local function finish(out)
    local output = (out.stdout or '') .. (out.stderr or '')
    require('hyperion.output').append(('$ %s\n%s'):format(line, output))
    if out.code == 0 then
      return on_done(true)
    end
    local tail = vim.list_slice(vim.split(vim.trim(output), '\n'), -15)
    client.notify(
      ('build failed (exit %d):\n%s'):format(out.code, table.concat(tail, '\n')),
      vim.log.levels.ERROR
    )
    on_done(false)
  end
  local ok, err =
    pcall(vim.system, cmd.argv, { cwd = cmd.cwd, text = true }, vim.schedule_wrap(finish))
  if not ok then
    client.notify(('cannot run `%s`: %s'):format(cmd.argv[1], err), vim.log.levels.ERROR)
    on_done(false)
  end
end

--- Resolve the `main` entry point under the cursor of the current buffer, build it, and return
--- `config` completed with the DAP `launch` arguments (or nothing, after telling the user why).
---@param dap_config table Configuration from nvim-dap (`type`, `request`, `name`, ...)
---@param on_config fun(config: table)
local function enrich(dap_config, on_config)
  if dap_config.request ~= 'launch' or dap_config.mainClass then
    return on_config(dap_config) -- hand-written configuration: nothing to resolve
  end
  local bufnr = vim.api.nvim_get_current_buf()
  local lsp = client.require(bufnr)
  if not lsp then
    return
  end
  local params = vim.lsp.util.make_position_params(0, lsp.offset_encoding)
  local arguments =
    { { uri = params.textDocument.uri, position = params.position, target = 'main' } }
  client.exec(lsp, 'hyperion.resolveLaunch', arguments, bufnr, function(spec)
    if type(spec) ~= 'table' or type(spec.launch) ~= 'table' then
      return client.notify('no `main` entry point found in this file.', vim.log.levels.WARN)
    end
    if type(spec.warnings) == 'table' and #spec.warnings > 0 then
      client.notify(table.concat(spec.warnings, '\n'), vim.log.levels.WARN)
    end
    build(spec, function(ok)
      if not ok then
        return
      end
      local launch = vim.deepcopy(spec.launch)
      if launch.env and next(launch.env) == nil then
        launch.env = nil -- an empty Lua table would be encoded as a JSON array
      end
      on_config(vim.tbl_extend('force', dap_config, launch, { name = PREFIX .. spec.name }))
    end)
  end)
end

---@param callback fun(adapter: table)
local function adapter(callback)
  local server = config.server() or config.binary
  if vim.fn.executable(server) == 0 then
    return client.notify(
      ('`%s` not found; cannot start the debug adapter.'):format(server),
      vim.log.levels.ERROR
    )
  end
  callback({ type = 'executable', command = server, args = { 'dap' }, enrich_config = enrich })
end

---@return table[]
local function default_configurations()
  return {
    {
      type = 'hyperion',
      request = 'launch',
      name = PREFIX .. 'launch the main method in this file',
    },
    {
      type = 'hyperion',
      request = 'attach',
      name = PREFIX .. 'attach to a JVM (JDWP)',
      host = '127.0.0.1',
      port = function()
        return tonumber(vim.fn.input('JDWP port: ', tostring(ATTACH_PORT))) or ATTACH_PORT
      end,
      projectRoot = function()
        local lsp = client.get(0, true)
        return lsp and lsp.root_dir or vim.fn.getcwd()
      end,
    },
  }
end

--- Register `dap.adapters.hyperion` and default Java/Kotlin configurations. Safe to call again;
--- it never replaces an adapter or configuration the user defined.
---@return boolean registered false when nvim-dap is not installed
function M.register()
  local dap = load()
  if not dap then
    return false
  end
  if dap.adapters.hyperion == nil then
    dap.adapters.hyperion = adapter
  end
  for _, filetype in ipairs({ 'java', 'kotlin' }) do
    local list = dap.configurations[filetype] or {}
    dap.configurations[filetype] = list
    if not vim.iter(list):any(function(c)
      return c.type == 'hyperion'
    end) then
      vim.list_extend(list, default_configurations())
    end
  end
  return true
end

--- `:Hyperion debug`: resolve, build and debug the `main` method of the current file.
function M.launch()
  if not M.register() then
    return client.notify('debugging needs nvim-dap, which is not installed.', vim.log.levels.WARN)
  end
  if not client.require(0) then
    return
  end
  require('dap').run({ type = 'hyperion', request = 'launch', name = PREFIX .. 'launch' })
end

return M
