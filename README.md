![zl logo](https://raw.githubusercontent.com/peri4n/zl/main/logo.png)

Status: WIP. API/UX unstable. Expect breaking changes.

A minimal, still-in-progress Zettelkasten CLI. Docs/features incomplete.

## Installation
- From crates.io: `cargo install zl --locked`
- From source: `cargo build --release`
- Run: `zl --help`

## Editor support
`zl lsp` runs a language server: go to definition on a link opens the linked note, references list
the links to a note, hover previews the linked note and completion offers notes to link to. Links to
missing notes are reported as diagnostics, headings and notes are available as symbols. Neovim (0.11
or later) starts it for markdown files in notebooks with:

```lua
vim.lsp.config('zl', { cmd = { 'zl', 'lsp' }, filetypes = { 'markdown' }, root_markers = { '.zl' } })
vim.lsp.enable('zl')
```

## Contributing
- Issues/PRs welcome
- Keep changes small, focused
- Be concise in discussions
- Use [conventional commits](https://www.conventionalcommits.org): they determine the next version
  and the changelog. Mark breaking changes with `!` (`feat!: ...`) or a `BREAKING CHANGE:` footer.

## License
GPL-3.0-or-later, see [LICENSE](LICENSE).
