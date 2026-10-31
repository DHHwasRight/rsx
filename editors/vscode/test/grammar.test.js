// The TextMate grammar, run the way VS Code runs it: vscode-textmate with the Oniguruma engine, and
// the Rust, HTML, CSS, and JavaScript grammars VS Code ships (from `tm-grammars`).
//
// Two kinds of checks:
// - scopes at chosen places in a sample that uses every construct;
// - every `.rsx` file in this repository: no `<tag` that is not a tag, and no JavaScript or CSS
//   scope outside `<script>` and `<style>` (an embedded rule that runs past its element).

const assert = require("assert");
const fs = require("fs");
const path = require("path");
const vsctm = require("vscode-textmate");
const oniguruma = require("vscode-oniguruma");

const root = path.resolve(__dirname, "..");
const repo = path.resolve(root, "../..");
const tm = (name) => JSON.parse(fs.readFileSync(require.resolve(`tm-grammars/grammars/${name}.json`), "utf8"));

async function loadGrammar() {
  await oniguruma.loadWASM(fs.readFileSync(require.resolve("vscode-oniguruma/release/onig.wasm")).buffer);
  const byScope = {
    "source.rsx": JSON.parse(fs.readFileSync(path.join(root, "syntaxes/rsx.tmLanguage.json"), "utf8")),
    "source.rust": tm("rust"),
    "source.css": tm("css"),
    "source.js": tm("javascript"),
  };
  const registry = new vsctm.Registry({
    onigLib: Promise.resolve({
      createOnigScanner: (s) => new oniguruma.OnigScanner(s),
      createOnigString: (s) => new oniguruma.OnigString(s),
    }),
    loadGrammar: async (scope) => byScope[scope] ?? null,
  });
  return registry.loadGrammar("source.rsx");
}

/// [{ line, text, scopes }] for every token of a source.
function tokenize(grammar, source) {
  let stack = vsctm.INITIAL;
  const out = [];
  source.split("\n").forEach((line, i) => {
    const r = grammar.tokenizeLine(line, stack);
    stack = r.ruleStack;
    for (const t of r.tokens) out.push({ line: i + 1, text: line.slice(t.startIndex, t.endIndex), scopes: t.scopes });
  });
  return out;
}

const SAMPLE = `use crate::models::User;

pub fn Page<'a>(user: &'a User, items: &[Item], children: Element) -> Element {
    let greeting = format!("Hi, {}", user.name);
    <!DOCTYPE html>
    <html lang="en">
        <head><style>body { color: red; }</style></head>
        <body class={body_class(user)} hidden?={user.banned} { ...extra }>
            <Layout title="Home" count={items.len()}>
                <p>Hello {greeting} &amp; welcome!</p>
                <div>{!user.bio_html}</div>
                if user.verified && items.len() > 2 {
                    <span class="badge">Verified</span>
                } else {
                    <span>none</span>
                }
                for item in items {
                    <Card title={&item.title} />
                }
                match user.status {
                    Status::Ok => <b>ok</b>,
                    Status::Err(e) => <i>{e}</i>,
                }
                <svg viewBox="0 0 10 10"><path stroke-width="2" d={d}></path></svg>
                <>{children}</>
            </Layout>
            <script>if (a < b && c) { go("x"); }</script>
        </body>
    </html>
}

fn body_class(user: &User) -> &'static str {
    if user.admin { "admin" } else { "user" }
}
`;

/// The scopes of the first token with this text on this line.
function scopesOf(tokens, line, text) {
  const t = tokens.find((t) => t.line === line && t.text.trim() === text);
  assert.ok(t, `no token "${text}" on line ${line}`);
  return t.scopes.join(" ");
}

function checkSample(grammar) {
  const tokens = tokenize(grammar, SAMPLE);
  const expect = [
    [3, "Page", "entity.name.function.component.rsx"],
    [4, "format!", "entity.name.function.macro.rust"],
    [5, "DOCTYPE", "entity.name.tag.html"],
    [7, "color", "source.css"],
    [8, "class", "entity.other.attribute-name.html"],
    [8, "body_class", "meta.embedded.expression.rust"],
    [8, "?", "keyword.operator.optional.rsx"],
    [8, "...", "keyword.operator.spread.rsx"],
    [9, "Layout", "support.class.component.rsx"],
    [10, "&amp;", "constant.character.entity.html"],
    [11, "!", "keyword.operator.unescaped.rsx"],
    [12, "if", "keyword.control.rust"],
    // Markup inside a control block is markup, not Rust.
    [13, "span", "entity.name.tag.html"],
    [13, "badge", "string.quoted.double.html"],
    [14, "else", "keyword.control.rust"],
    [15, "span", "entity.name.tag.html"],
    [18, "Card", "support.class.component.rsx"],
    [21, "Ok", "entity.name.type"],
    [21, "b", "entity.name.tag.html"],
    [24, "stroke-width", "entity.other.attribute-name.html"],
    [26, "Layout", "support.class.component.rsx"],
    [27, "go", "source.js"],
    // After a one-line script, the closing tags are tags again.
    [28, "body", "entity.name.tag.html"],
    // A plain function is Rust: the first `}` does not end it.
    [33, "if", "keyword.control.rust"],
    [33, "else", "keyword"],
  ];
  for (const [line, text, scope] of expect) {
    const scopes = scopesOf(tokens, line, text);
    assert.ok(scopes.includes(scope), `line ${line} "${text}": expected ${scope}, got ${scopes}`);
  }
  // Text in markup is text: a capitalized word is not a Rust type.
  assert.ok(!scopesOf(tokens, 10, "Hello").includes("entity.name.type"), "markup text scoped as a type");
}

function rsxFiles(dir, out = []) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (["target", "node_modules", ".git", ".vscode-test"].includes(entry.name)) continue;
    const p = path.join(dir, entry.name);
    if (entry.isDirectory()) rsxFiles(p, out);
    else if (entry.name.endsWith(".rsx")) out.push(p);
  }
  return out;
}

function checkRepository(grammar) {
  const problems = [];
  const files = rsxFiles(repo);
  for (const file of files) {
    let inRaw = false;
    let stack = vsctm.INITIAL;
    fs.readFileSync(file, "utf8")
      .split("\n")
      .forEach((line, i) => {
        const r = grammar.tokenizeLine(line, stack);
        stack = r.ruleStack;
        if (/<(script|style)\b/.test(line)) inRaw = true;
        for (const t of r.tokens) {
          const rest = line.slice(t.startIndex);
          const scopes = t.scopes.join(" ");
          const tagNotTag = /^<\/?[A-Za-z]/.test(rest) && !/tag|comment|string|angle|relational/.test(scopes);
          const leak = !inRaw && /source\.(js|css)/.test(scopes);
          if (tagNotTag || leak) {
            problems.push(`${path.relative(repo, file)}:${i + 1}: ${line.trim()}`);
            break;
          }
        }
        if (/<\/(script|style)\s*>/.test(line)) inRaw = false;
      });
  }
  assert.ok(files.length > 0, "no .rsx files found");
  assert.deepStrictEqual(problems, [], `mis-scoped lines:\n${problems.join("\n")}`);
  return files.length;
}

(async () => {
  const grammar = await loadGrammar();
  checkSample(grammar);
  const n = checkRepository(grammar);
  console.log(`grammar: sample scopes ok, ${n} .rsx files ok`);
})().catch((err) => {
  console.error(err.message);
  process.exit(1);
});
