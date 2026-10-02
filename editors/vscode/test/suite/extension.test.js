// What an editor actually gets from the extension.
//
// These run inside a real VS Code, so they go through activation, the
// language client, the stdio transport and the server — the parts that unit
// tests in Rust cannot reach.

const assert = require("assert");
const path = require("path");
const vscode = require("vscode");

const FIXTURES = path.resolve(__dirname, "..", "fixtures", "views");

function uri(name) {
  return vscode.Uri.file(path.join(FIXTURES, name));
}

async function open(name) {
  const document = await vscode.workspace.openTextDocument(uri(name));
  await vscode.window.showTextDocument(document, { preview: false });
  return document;
}

/// Polls until `check` returns something truthy, or the budget runs out.
///
/// The language client starts asynchronously and indexes in the background,
/// so the first request after opening a file can legitimately come back empty.
async function eventually(check, what, budgetMs = 30000) {
  const deadline = Date.now() + budgetMs;
  let last;
  while (Date.now() < deadline) {
    last = await check();
    if (last !== undefined && last !== null && (!Array.isArray(last) || last.length > 0)) {
      return last;
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error(`timed out waiting for ${what}; last value: ${JSON.stringify(last)}`);
}

suite("rsx extension", function () {
  suiteSetup(async function () {
    const extension = vscode.extensions.getExtension("rsx.rsx");
    assert.ok(extension, "extension rsx.rsx is not installed");
    const api = await extension.activate();

    assert.ok(
      api && api.started,
      `language server did not start (path: ${api && api.serverPath}, ` +
        `error: ${api && api.startError && api.startError.message})`
    );
    // Opening a file is what starts the server; give it one.
    await open("card.rsx");
    await eventually(
      async () =>
        (await vscode.commands.executeCommand(
          "vscode.executeDocumentSymbolProvider",
          uri("card.rsx")
        )) || null,
      "the language server to answer"
    );
  });

  test("recognises .rsx as its own language", async () => {
    const document = await open("card.rsx");
    assert.strictEqual(document.languageId, "rsx");
  });

  test("reports diagnostics for a broken template", async () => {
    await open("broken.rsx");
    const diagnostics = await eventually(
      async () => vscode.languages.getDiagnostics(uri("broken.rsx")),
      "diagnostics on broken.rsx"
    );

    assert.ok(diagnostics.length > 0, "expected a diagnostic");
    assert.strictEqual(diagnostics[0].source, "rsx");
    assert.match(diagnostics[0].message, /does not match/);
    // The caret belongs on the closing tag, which is line 1.
    assert.strictEqual(diagnostics[0].range.start.line, 1);
  });

  test("reports no diagnostics for a valid template", async () => {
    await open("card.rsx");
    // Give the server a chance to publish before concluding there are none.
    await new Promise((resolve) => setTimeout(resolve, 1500));
    assert.deepStrictEqual(vscode.languages.getDiagnostics(uri("card.rsx")), []);
  });

  test("lists components as document symbols", async () => {
    await open("card.rsx");
    const symbols = await eventually(
      async () =>
        vscode.commands.executeCommand(
          "vscode.executeDocumentSymbolProvider",
          uri("card.rsx")
        ),
      "document symbols"
    );

    const names = symbols.map((s) => s.name);
    assert.ok(names.includes("Card"), `got ${names}`);
    assert.ok(names.includes("Panel"), `got ${names}`);

    const card = symbols.find((s) => s.name === "Card");
    assert.ok(
      card.children.some((c) => c.name === "title"),
      "expected parameters as children"
    );
  });

  test("hovers a component definition", async () => {
    await open("card.rsx");
    // `Card` on line 6 of card.rsx.
    const hovers = await eventually(
      async () =>
        vscode.commands.executeCommand(
          "vscode.executeHoverProvider",
          uri("card.rsx"),
          new vscode.Position(6, 9)
        ),
      "a hover on Card"
    );

    const text = hovers
      .flatMap((h) => h.contents.map((c) => (typeof c === "string" ? c : c.value)))
      .join("\n");
    assert.match(text, /title: &str/);
    assert.match(text, /A card with a title\./);
  });

  test("hovers an HTML element", async () => {
    await open("card.rsx");
    const hovers = await eventually(
      async () =>
        vscode.commands.executeCommand(
          "vscode.executeHoverProvider",
          uri("card.rsx"),
          new vscode.Position(7, 6)
        ),
      "a hover on div"
    );

    const text = hovers
      .flatMap((h) => h.contents.map((c) => (typeof c === "string" ? c : c.value)))
      .join("\n");
    assert.match(text, /block container/);
  });

  test("jumps from a component call to its declaration", async () => {
    await open("page.rsx");
    // `<Card` on line 4 of page.rsx.
    const locations = await eventually(
      async () =>
        vscode.commands.executeCommand(
          "vscode.executeDefinitionProvider",
          uri("page.rsx"),
          new vscode.Position(4, 10)
        ),
      "a definition for Card"
    );

    assert.ok(locations.length > 0, "expected a location");
    const target = locations[0].targetUri || locations[0].uri;
    assert.ok(target.fsPath.endsWith("card.rsx"), `landed on ${target.fsPath}`);
  });

  test("completes components and HTML elements after <", async () => {
    const document = await open("page.rsx");
    const editor = vscode.window.activeTextEditor;

    // Type a bare `<` on its own line inside Page.
    const insertAt = new vscode.Position(5, 0);
    await editor.edit((builder) => builder.insert(insertAt, "        <\n"));

    try {
      const list = await eventually(
        async () =>
          vscode.commands.executeCommand(
            "vscode.executeCompletionItemProvider",
            document.uri,
            new vscode.Position(5, 9)
          ),
        "completions after <"
      );

      const labels = list.items.map((i) =>
        typeof i.label === "string" ? i.label : i.label.label
      );
      assert.ok(labels.includes("Card"), `no Card in ${labels.slice(0, 20)}`);
      assert.ok(labels.includes("div"), `no div in ${labels.slice(0, 20)}`);

      // Components sort ahead of HTML tags.
      const card = list.items.find((i) => (i.label.label || i.label) === "Card");
      const div = list.items.find((i) => (i.label.label || i.label) === "div");
      assert.ok(
        card.sortText < div.sortText,
        `expected Card (${card.sortText}) before div (${div.sortText})`
      );
    } finally {
      await vscode.commands.executeCommand("undo");
    }
  });

  test("completes a component's props inside its tag", async () => {
    const document = await open("page.rsx");
    const editor = vscode.window.activeTextEditor;
    const insertAt = new vscode.Position(5, 0);
    await editor.edit((builder) => builder.insert(insertAt, "        <Card \n"));

    try {
      const list = await eventually(
        async () =>
          vscode.commands.executeCommand(
            "vscode.executeCompletionItemProvider",
            document.uri,
            new vscode.Position(5, 14)
          ),
        "prop completions"
      );

      const labels = list.items.map((i) =>
        typeof i.label === "string" ? i.label : i.label.label
      );
      assert.ok(labels.includes("title"), `no title in ${labels}`);
      assert.ok(labels.includes("count"), `no count in ${labels}`);
    } finally {
      await vscode.commands.executeCommand("undo");
    }
  });

  test("updates diagnostics as the document changes", async () => {
    const document = await open("card.rsx");
    const editor = vscode.window.activeTextEditor;

    // Break it.
    await editor.edit((builder) =>
      builder.insert(new vscode.Position(0, 0), "pub fn Bad() -> Element { <div> }\n")
    );
    const broken = await eventually(
      async () => vscode.languages.getDiagnostics(document.uri),
      "a diagnostic after breaking the document"
    );
    assert.ok(broken.length > 0);

    // Fix it.
    await vscode.commands.executeCommand("undo");
    await eventually(
      async () => {
        const current = vscode.languages.getDiagnostics(document.uri);
        return current.length === 0 ? "cleared" : null;
      },
      "diagnostics to clear after undo"
    );
  });
});
