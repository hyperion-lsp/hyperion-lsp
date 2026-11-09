-- Copyright 2026 Adrian Helvik
-- SPDX-License-Identifier: PolyForm-Shield-1.0.0

--- The `:Hyperion` user command.

local client = require('hyperion.client')
local config = require('hyperion.config')

local notify = client.notify
local WARN = vim.log.levels.WARN

local M = {}

---@param cmd string
---@return string? output
local function capture(cmd, ...)
  local ok, proc = pcall(vim.system, { cmd, ... }, { text = true })
  local out = ok and proc:wait(3000) or nil
  if out and out.code == 0 then
    return vim.trim(out.stdout .. out.stderr)
  end
end

--- `hyperion.runConfigurations` carries no source location, so ask the workspace index for the
--- file that declares `class_name`. Calls `on_uri(nil)` when it cannot be found.
---@param lsp vim.lsp.Client
---@param class_name string binary name, e.g. `com.example.Main`
---@param on_uri fun(uri?: string)
local function locate_source(lsp, class_name, on_uri)
  local outer = class_name:match('^[^$]+')
  local simple = outer:match('[^.]+$')
  local suffix = '/' .. outer:gsub('%.', '/')
  lsp:request('workspace/symbol', { query = simple }, function(_, symbols)
    for _, symbol in ipairs(symbols or {}) do
      local uri = symbol.location and symbol.location.uri
      local path = uri and vim.uri_to_fname(uri)
      if
        path and (vim.endswith(path, suffix .. '.java') or vim.endswith(path, suffix .. '.kt'))
      then
        return on_uri(uri)
      end
    end
    on_uri(nil)
  end)
end

---@param lsp vim.lsp.Client
---@param run_config table
local function run_configuration(lsp, run_config)
  locate_source(lsp, run_config.mainClass, function(uri)
    if not uri then
      return notify(('cannot find the source file of %s.'):format(run_config.mainClass), WARN)
    end
    notify(('running %s (output: :Hyperion output)'):format(run_config.mainClass))
    client.exec(
      lsp,
      'hyperion.run',
      { { className = run_config.mainClass, uri = uri } },
      0,
      function() end
    )
  end)
end

local commands = {}

--- Re-read the Gradle/Maven model.
function commands.reload()
  local lsp = client.require(0, true)
  if lsp then
    client.exec(lsp, 'hyperion.reloadProject', nil, 0, function()
      notify('reloading the project model.')
    end)
  end
end

--- Pick an IntelliJ run configuration (`.run/*.xml`, `.idea/runConfigurations`) and run its main
--- class in the server's VM. `:Hyperion run {name}` skips the picker.
function commands.run(args)
  local lsp = client.require(0, true)
  if not lsp then
    return
  end
  client.exec(lsp, 'hyperion.runConfigurations', nil, 0, function(result)
    local choices = vim.tbl_filter(function(item)
      return item.request == 'launch' and item.mainClass
    end, result or {})
    if #choices == 0 then
      return notify(
        'no application run configurations found (.run/*.xml or .idea/runConfigurations).',
        WARN
      )
    end
    if #args > 0 then
      local name = table.concat(args, ' ')
      for _, item in ipairs(choices) do
        if item.name == name then
          return run_configuration(lsp, item)
        end
      end
      return notify(('no run configuration named "%s".'):format(name), WARN)
    end
    vim.ui.select(choices, {
      prompt = 'Run configuration:',
      format_item = function(item)
        return ('%s  (%s)'):format(item.name, item.mainClass)
      end,
    }, function(item)
      if item then
        run_configuration(lsp, item)
      end
    end)
  end)
end

--- Build and debug the `main` method under the cursor with nvim-dap.
function commands.debug()
  require('hyperion.dap').launch()
end
commands.launch = commands.debug

--- Run the code lens ("Run") on the cursor line.
function commands.codelens()
  local lsp = client.require(0)
  if lsp then
    vim.lsp.codelens.run({ client_id = lsp.id })
  end
end

--- Show the server's `window/logMessage` output, including what a Run lens printed.
function commands.output()
  require('hyperion.output').open()
end

--- Open Neovim's LSP log.
function commands.log()
  vim.cmd.edit(vim.fn.fnameescape(vim.lsp.log.get_filename()))
end

--- Summarize the server binary and the client attached to this buffer.
function commands.info()
  local server = config.server()
  local lines = { 'Hyperion' }
  local function add(label, value)
    lines[#lines + 1] = ('  %-12s %s'):format(label, value)
  end
  add(
    'server',
    server and (vim.fn.exepath(server) ~= '' and vim.fn.exepath(server) or server .. ' (not found)')
      or 'custom cmd'
  )
  local version = server and capture(server, '--version')
  if version then
    add('version', version)
  end
  add('enabled', tostring(config.enabled()))
  local lsp = client.get(0, true)
  if lsp then
    add('client', ('id %d, root %s'):format(lsp.id, lsp.root_dir or 'none'))
    add('buffers', table.concat(vim.tbl_keys(lsp.attached_buffers), ' '))
  else
    add('client', 'not attached to this buffer')
  end
  add('nvim-dap', require('hyperion.dap').available() and 'installed' or 'not installed')
  add('lsp log', vim.lsp.log.get_filename())
  vim.api.nvim_echo({ { table.concat(lines, '\n') } }, true, {})
end

local names = { 'reload', 'run', 'debug', 'launch', 'codelens', 'output', 'log', 'info' }

---@param fargs string[]
function M.run(fargs)
  local name = fargs[1]
  local command = name and commands[name]
  if not command then
    local message = name and ('unknown subcommand "%s". '):format(name) or ''
    return notify(message .. 'usage: :Hyperion {' .. table.concat(names, '|') .. '}', WARN)
  end
  command(vim.list_slice(fargs, 2))
end

---@param arglead string
---@param cmdline string
---@return string[]
function M.complete(arglead, cmdline)
  local words = vim.split(cmdline, '%s+', { trimempty = true })
  if #words > 2 or (#words == 2 and cmdline:match('%s$')) then
    return {}
  end
  return vim.tbl_filter(function(name)
    return vim.startswith(name, arglead)
  end, names)
end

return M
