// Builds the .vsix from a production-only install.
//
// Packaging straight from the working tree does not work: `vsce` derives the
// runtime dependency set itself, and with devDependencies installed it drops
// transitive ones that are shared with them — `minimatch`, reached through
// the language client, went missing that way and the published extension
// failed to activate.
//
// Copying the extension into a temp directory and running `npm ci --omit=dev`
// there leaves a tree that is exactly the runtime closure, so there is
// nothing for `vsce` to get wrong, and the repository's own node_modules is
// left alone.

const { execFileSync } = require("child_process");
const fs = require("fs");
const os = require("os");
const path = require("path");

const root = path.resolve(__dirname, "..");

/// Files that make up the extension itself, as opposed to its tests.
const SOURCES = [
  "extension.js",
  "package.json",
  "package-lock.json",
  "language-configuration.json",
  "syntaxes",
  "README.md",
  ".vscodeignore",
];

function build(outputVsix) {
  const staging = fs.mkdtempSync(path.join(os.tmpdir(), "rsx-vsix-"));

  for (const name of SOURCES) {
    const from = path.join(root, name);
    if (fs.existsSync(from)) {
      fs.cpSync(from, path.join(staging, name), { recursive: true });
    }
  }

  execFileSync("npm", ["ci", "--omit=dev", "--no-audit", "--no-fund"], {
    cwd: staging,
    stdio: "pipe",
  });
  execFileSync(
    "npx",
    ["--yes", "@vscode/vsce", "package", "--allow-missing-repository", "--skip-license", "-o", outputVsix],
    { cwd: staging, stdio: "pipe" }
  );

  fs.rmSync(staging, { recursive: true, force: true });
  return outputVsix;
}

module.exports = { build };

if (require.main === module) {
  const out = process.argv[2] || path.join(root, "rsx-0.1.0.vsix");
  build(out);
  const size = (fs.statSync(out).size / 1024).toFixed(0);
  console.log(`packaged ${path.basename(out)} (${size} KB)`);
}
