-- Copyright 2026 Adrian Helvik
-- SPDX-License-Identifier: PolyForm-Shield-1.0.0

local config = require('hyperion.config')

local M = {}

--- Apply `opts` and, unless `auto_enable = false`, enable the server. Calling it is optional.
---@param opts? hyperion.Options
function M.setup(opts)
  vim.validate('opts', opts, 'table', true)
  local options = vim.tbl_deep_extend('force', config.defaults, opts or {})
  config.options = options

  local override = {}
  if options.server_path then
    override.cmd = { vim.fn.expand(options.server_path) }
  end
  if #options.extra_filetypes > 0 then
    override.filetypes = vim.list_extend(vim.deepcopy(config.filetypes), options.extra_filetypes)
  end
  -- The only server setting today: running Gradle/Maven executes the project's build scripts, so
  -- it is sent only when the user asks for it. The server reads it from either place.
  local function with_build_tool(base)
    if options.build_tool_classpath == nil then
      return base
    end
    local flag = { hyperion = { buildToolClasspath = options.build_tool_classpath } }
    return vim.tbl_deep_extend('force', base or {}, flag)
  end
  override.init_options = with_build_tool(options.init_options)
  override.settings = with_build_tool(options.settings)
  vim.lsp.config(config.name, override)

  if options.auto_enable then
    local server = config.server()
    if server and vim.fn.executable(server) == 0 then
      require('hyperion.client').notify(
        (
          '`%s` was not found, so the server is not enabled. Put it on $PATH or pass '
          .. 'server_path to setup(). See :checkhealth hyperion.'
        ):format(server),
        vim.log.levels.WARN
      )
    else
      vim.lsp.enable(config.name)
    end
  end
end

--- Called from the `LspAttach` autocmd in `plugin/hyperion.lua`, so it also runs without `setup()`.
---@param client vim.lsp.Client
---@param bufnr integer
function M.on_attach(client, bufnr)
  if config.options.dap then
    require('hyperion.dap').register()
  end
  if not config.options.codelens or not client:supports_method('textDocument/codeLens') then
    return
  end
  if vim.lsp.codelens.enable then
    vim.lsp.codelens.enable(true, { bufnr = bufnr, client_id = client.id })
    return
  end
  -- Neovim 0.11 has no automatic refresh.
  local group = vim.api.nvim_create_augroup('hyperion.codelens.' .. bufnr, { clear = true })
  vim.api.nvim_create_autocmd({ 'BufEnter', 'CursorHold', 'InsertLeave' }, {
    group = group,
    buffer = bufnr,
    callback = function()
      vim.lsp.codelens.refresh({ bufnr = bufnr })
    end,
  })
  vim.api.nvim_create_autocmd('LspDetach', {
    group = group,
    buffer = bufnr,
    once = true,
    callback = function()
      vim.api.nvim_del_augroup_by_id(group)
    end,
  })
  vim.lsp.codelens.refresh({ bufnr = bufnr })
end

return M
