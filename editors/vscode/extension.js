const vscode = require("vscode");
const { LanguageClient, TransportKind, State } = require("vscode-languageclient/node");

let client;

async function activate(context) {
  // Syntax highlighting is declarative and works anywhere. The language
  // server is not: it runs a binary whose path can come from workspace
  // settings, which is exactly the kind of thing Workspace Trust exists to
  // gate. So in an untrusted workspace the extension still loads — colours,
  // brackets, indentation — and the server waits for trust to be granted.
  if (!vscode.workspace.isTrusted) {
    const granted = vscode.workspace.onDidGrantWorkspaceTrust(() => {
      granted.dispose();
      start(context);
    });
    context.subscriptions.push(granted);
    return { client: undefined, started: false, awaitingTrust: true };
  }

  return start(context);
}

async function start(context) {
  const config = vscode.workspace.getConfiguration("rsx");
  const serverPath = config.get("server.path", "rsx");

  if (!config.get("server.enabled", true)) {
    return { client: undefined, serverPath, started: false };
  }

  const serverOptions = {
    command: serverPath,
    args: ["lsp"],
    transport: TransportKind.stdio,
  };

  const clientOptions = {
    documentSelector: [{ scheme: "file", language: "rsx" }],
    outputChannel: vscode.window.createOutputChannel("RSX Language Server"),
    // A cursor inside `{ user.posts.| }` is asking a Rust question, and the
    // server forwards those to rust-analyzer rather than guessing.
    initializationOptions: {
      rustAnalyzer: {
        enabled: config.get("rustAnalyzer.enabled", true),
        path: config.get("rustAnalyzer.path", "rust-analyzer"),
      },
    },
  };

  client = new LanguageClient("rsx", "RSX Language Server", serverOptions, clientOptions);
  context.subscriptions.push(client);

  let startError;
  try {
    await client.start();
  } catch (error) {
    // A missing binary is the common first-run failure; say so once rather
    // than letting it surface as an unhandled rejection.
    startError = error;
    vscode.window.showWarningMessage(
      `rsx: could not start the language server at "${serverPath}" (${error.message}). ` +
        "Set rsx.server.path, or turn off rsx.server.enabled for highlighting only."
    );
  }

  // Exported so a test can assert the server really came up, and report the
  // path it tried when it did not.
  return {
    client,
    serverPath,
    started: client.state === State.Running,
    startError,
    awaitingTrust: false,
  };
}

function deactivate() {
  return client ? client.stop() : undefined;
}

module.exports = { activate, deactivate };
