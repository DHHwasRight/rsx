const vscode = require("vscode");
const { LanguageClient, TransportKind } = require("vscode-languageclient/node");

let client;

function activate(context) {
  const config = vscode.workspace.getConfiguration("rsx");
  if (!config.get("server.enabled", true)) {
    return;
  }

  const serverOptions = {
    command: config.get("server.path", "rsx"),
    args: ["lsp"],
    transport: TransportKind.stdio,
  };

  const clientOptions = {
    documentSelector: [{ scheme: "file", language: "rsx" }],
  };

  client = new LanguageClient("rsx", "RSX Language Server", serverOptions, clientOptions);
  // `start` rejects when the binary is missing, which is the common first-run
  // case; say so once instead of letting it surface as an unhandled rejection.
  client.start().catch((error) => {
    vscode.window.showWarningMessage(
      `rsx: could not start the language server (${error.message}). ` +
        "Set rsx.server.path, or turn off rsx.server.enabled for highlighting only."
    );
  });
  context.subscriptions.push(client);
}

function deactivate() {
  return client ? client.stop() : undefined;
}

module.exports = { activate, deactivate };
