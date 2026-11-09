-- Copyright 2026 Adrian Helvik
-- SPDX-License-Identifier: PolyForm-Shield-1.0.0

if vim.g.loaded_hyperion then
  return
end
vim.g.loaded_hyperion = 1

if vim.fn.has('nvim-0.11') == 0 then
  vim.notify('hyperion.nvim requires Neovim 0.11 or newer', vim.log.levels.WARN)
  return
end

vim.api.nvim_create_user_command('Hyperion', function(opts)
  require('hyperion.command').run(opts.fargs)
end, {
  nargs = '*',
  desc = 'Hyperion language server: reload, run, debug, codelens, output, log, info',
  complete = function(arglead, cmdline)
    return require('hyperion.command').complete(arglead, cmdline)
  end,
})

vim.api.nvim_create_autocmd('LspAttach', {
  group = vim.api.nvim_create_augroup('hyperion', { clear = true }),
  desc = 'Hyperion: code lens refresh and nvim-dap registration',
  callback = function(args)
    local client = vim.lsp.get_client_by_id(args.data.client_id)
    if client and client.name == 'hyperion' then
      require('hyperion').on_attach(client, args.buf)
    end
  end,
})
