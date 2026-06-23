# Pyrst for Visual Studio Code

Language support for [Pyrst](https://github.com/pyrst/pyrst) (`.pyrs` files) — a Pythonic syntax that compiles to Rust.

## Features

- **Syntax highlighting** for `.pyrs` files: keywords, control flow, constants, type names (including `Mut[T]`), decorators (`@property`, `@staticmethod`, `@dataclass`), f-strings with interpolation highlighting, numbers, and comments.
- **Live diagnostics** — parse errors and type errors are published by the `pyrst lsp` language server and shown as red squiggles in the editor.

  > **Note:** The language server currently publishes **one diagnostic at a time** (the first error it encounters). Full multi-error reporting will be available once the collect-all-diagnostics card lands.

- **Format Document** / format-on-save — delegates to the same formatter as `pyrst fmt`.

## Requirements

The `pyrst` binary must be available. There are two ways to satisfy this:

1. **Add `pyrst` to your PATH** — the extension will find it automatically.
2. **Set `pyrst.server.path`** in your VS Code settings to the absolute path of the `pyrst` executable, e.g.:
   ```json
   {
     "pyrst.server.path": "/home/you/.cargo/bin/pyrst"
   }
   ```

## Extension Settings

| Setting | Default | Description |
|---------|---------|-------------|
| `pyrst.server.path` | `"pyrst"` | Path to the `pyrst` executable used to start the language server (`pyrst lsp`). |
| `pyrst.trace.server` | `"off"` | Set to `"messages"` or `"verbose"` to trace LSP traffic between VS Code and the server. Useful for debugging. |

## Installing (development / local)

1. Build the extension:
   ```bash
   cd editors/vscode
   npm install
   npm run compile
   ```

2. **Option A — F5 dev-host:** Open the `editors/vscode` folder in VS Code and press **F5** to launch a new Extension Development Host window with the extension loaded.

3. **Option B — package and install:**
   ```bash
   # install vsce if you don't have it
   npm install -g @vscode/vsce
   vsce package
   code --install-extension pyrst-0.1.0.vsix
   ```

## Known Limitations

- One diagnostic at a time (first error only). This is a language-server limitation, not an editor one — see the collect-all-diagnostics card.
- Hover, completion, and go-to-definition are not yet implemented in the language server.

## License

MIT
