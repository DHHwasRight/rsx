# RSX for Visual Studio Code

Syntax highlighting and language-server support for `.rsx` templates.

## What it does

Highlighting works on its own. Everything else comes from the `rsx` language
server, which the extension starts for you:

- diagnostics as you type, including the ones only code generation finds, such
  as a prop a component does not declare
- hover for components, parameters, elements and interpolated expressions
- completion after `<` for components and HTML elements, and inside a tag for
  props and attributes
- go to definition on a component call
- an outline of the components a file declares
- field and method completion inside `{ … }` expressions, with real types,
  by forwarding the question to `rust-analyzer`

## Installing

The extension needs the `rsx` binary on `PATH`:

```sh
cargo install --path crates/rsx-cli
```

Then, from this directory:

```sh
npm install
npx vsce package
code --install-extension rsx-0.1.0.vsix
```

For development, open this folder in VS Code and press F5.

## Settings

| Setting | Default | Meaning |
| --- | --- | --- |
| `rsx.server.path` | `rsx` | Path to the binary providing the language server |
| `rsx.server.enabled` | `true` | Set to `false` for highlighting without the server |
| `rsx.rustAnalyzer.enabled` | `true` | Consult `rust-analyzer` for Rust expressions |
| `rsx.rustAnalyzer.path` | `rust-analyzer` | Which analyzer binary to run |

## How Rust questions are answered

A cursor inside `{ user.posts.| }` is asking about Rust, not about markup. The
server maps the position into the generated Rust — byte for byte, through the
source map — and forwards the question to `rust-analyzer`, which is already
analysing your crate.

It degrades quietly: with no `rust-analyzer` installed, or for a template that
belongs to no crate, you still get everything else.

## Testing

```sh
npm test
```

This launches a real VS Code and drives the extension through the editor API.
