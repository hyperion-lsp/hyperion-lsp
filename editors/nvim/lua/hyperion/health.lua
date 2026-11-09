-- Copyright 2026 Adrian Helvik
-- SPDX-License-Identifier: PolyForm-Shield-1.0.0

--- `:checkhealth hyperion`

local config = require('hyperion.config')

local health = vim.health
local M = {}

---@param cmd string[]
---@return string? output first line of stdout/stderr, nil when the command failed
local function first_line(cmd)
  local ok, proc = pcall(vim.system, cmd, { text = true })
  local out = ok and proc:wait(5000) or nil
  if out and out.code == 0 then
    return vim.split(vim.trim(out.stdout .. out.stderr), '\n')[1]
  end
end

local function check_neovim()
  local v = vim.version()
  local text = ('Neovim %d.%d.%d'):format(v.major, v.minor, v.patch)
  if vim.fn.has('nvim-0.11') == 1 then
    health.ok(text)
  else
    health.error(text .. ': Neovim 0.11 or newer is required (vim.lsp.config / vim.lsp.enable)')
  end
end

local function check_server()
  local server = config.server()
  if not server then
    return health.info('`cmd` is a function; the server binary cannot be checked')
  end
  local path = vim.fn.exepath(server)
  if path == '' then
    return health.error(('`%s` not found'):format(server), {
      'Build it from source and put it on $PATH, or pass `server_path` to setup()',
      'https://github.com/hyperion-lsp/hyperion-lsp#try-it',
    })
  end
  health.ok('server: ' .. path)
  local version = first_line({ path, '--version' })
  if version then
    health.ok('version: ' .. version)
  else
    health.warn('`' .. path .. ' --version` failed; the binary may be broken or too old')
  end
end

local function check_java()
  local java = vim.fn.exepath('java')
  if java == '' and vim.env.JAVA_HOME and vim.env.JAVA_HOME ~= '' then
    java = vim.fs.joinpath(vim.env.JAVA_HOME, 'bin', 'java')
    java = vim.fn.executable(java) == 1 and java or ''
  end
  if java == '' then
    return health.warn('no JDK found on $PATH or $JAVA_HOME', {
      'Hyperion reads the JDK for standard-library navigation, and debugging launches `java`',
    })
  end
  local version = first_line({ java, '-version' })
  health.ok(('java: %s (%s)'):format(java, version or 'version unknown'))
end

local function check_dap()
  if require('hyperion.dap').available() then
    local dap = require('dap')
    health.ok(
      'nvim-dap installed' .. (dap.adapters.hyperion and ', hyperion adapter registered' or '')
    )
  else
    health.info('nvim-dap not installed (optional; needed for :Hyperion debug)')
  end
end

local function check_client()
  if config.enabled() == false then
    health.warn('`hyperion` is not enabled', {
      'Call vim.lsp.enable("hyperion") or require("hyperion").setup()',
    })
  else
    health.ok(
      '`hyperion` is enabled for: '
        .. table.concat(vim.lsp.config[config.name].filetypes or {}, ', ')
    )
  end
  local clients = vim.lsp.get_clients({ name = config.name })
  if #clients == 0 then
    return health.info('no running client (open a Java, Kotlin or Groovy file in a project)')
  end
  for _, c in ipairs(clients) do
    health.ok(('client %d, root %s'):format(c.id, c.root_dir or 'none'))
  end
end

function M.check()
  health.start('hyperion.nvim')
  check_neovim()
  check_server()
  check_java()
  check_dap()
  check_client()
end

return M
