// The packaged extension has to contain everything it needs to load.
//
// Hand-listing runtime dependencies in `.vscodeignore` once shipped a package
// missing `minimatch`, a transitive dependency of the language client. The
// extension installed cleanly, appeared in the Extensions view, and then
// failed to activate on the user's machine with nothing but a stack of module
// paths. The VS Code suite cannot catch that: it runs the extension from
// source, where every dependency is present.
//
// So this builds the real package and checks it the way Node would — every
// file the manifest names, and the whole dependency graph of the entry point.

const assert = require("assert");
const { execFileSync } = require("child_process");
const fs = require("fs");
const os = require("os");
const path = require("path");

const root = path.resolve(__dirname, "..");

function packageExtension(into) {
  const vsix = require("../scripts/build-package").build(path.join(into, "rsx-test.vsix"));
  execFileSync("unzip", ["-q", vsix, "-d", into], { stdio: "pipe" });
  return path.join(into, "extension");
}

/// Resolves a dependency the way Node does: nearest `node_modules` first.
function resolvePackage(name, fromDir, packageRoot) {
  let dir = fromDir;
  for (;;) {
    const candidate = path.join(dir, "node_modules", name);
    if (fs.existsSync(path.join(candidate, "package.json"))) {
      return candidate;
    }
    if (path.resolve(dir) === path.resolve(packageRoot)) {
      return null;
    }
    const parent = path.dirname(dir);
    if (parent === dir) {
      return null;
    }
    dir = parent;
  }
}

/// Walks the runtime dependency graph, collecting anything that is absent.
function missingDependencies(packageRoot) {
  const missing = [];
  const seen = new Set();

  const visit = (pkgDir, trail) => {
    if (seen.has(pkgDir)) return;
    seen.add(pkgDir);

    const manifest = JSON.parse(fs.readFileSync(path.join(pkgDir, "package.json"), "utf8"));
    for (const name of Object.keys(manifest.dependencies || {})) {
      const resolved = resolvePackage(name, pkgDir, packageRoot);
      if (!resolved) {
        missing.push(`${[...trail, name].join(" -> ")}`);
        continue;
      }
      visit(resolved, [...trail, name]);
    }
  };

  visit(packageRoot, [path.basename(packageRoot)]);
  return missing;
}

function run() {
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), "rsx-pkg-"));
  try {
    const dir = packageExtension(temp);
    const manifest = JSON.parse(fs.readFileSync(path.join(dir, "package.json"), "utf8"));

    // Everything the manifest names must actually be in the package.
    assert.ok(fs.existsSync(path.join(dir, manifest.main)), `main is missing: ${manifest.main}`);
    for (const grammar of manifest.contributes.grammars) {
      assert.ok(fs.existsSync(path.join(dir, grammar.path)), `grammar missing: ${grammar.path}`);
    }
    for (const language of manifest.contributes.languages) {
      assert.ok(
        fs.existsSync(path.join(dir, language.configuration)),
        `language configuration missing: ${language.configuration}`
      );
    }

    // The extension must still load in a restricted workspace, so syntax
    // highlighting survives there even though the server does not start.
    assert.strictEqual(
      manifest.capabilities.untrustedWorkspaces.supported,
      "limited",
      "the extension must declare how it behaves in an untrusted workspace"
    );

    // The dependency graph has to be complete.
    const missing = missingDependencies(dir);
    assert.deepStrictEqual(missing, [], `packaged extension is missing dependencies:\n  ${missing.join("\n  ")}`);

    // And the dev dependencies have to be absent, or the package is huge.
    for (const dev of Object.keys(require("../package.json").devDependencies || {})) {
      assert.ok(
        !fs.existsSync(path.join(dir, "node_modules", dev)),
        `devDependency ${dev} was packaged`
      );
    }

    console.log(`package test: ok (${Object.keys(manifest.contributes).length} contribution points)`);
  } finally {
    fs.rmSync(temp, { recursive: true, force: true });
  }
}

run();
