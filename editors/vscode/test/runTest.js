// Launches a real VS Code with this extension loaded and runs the suite
// inside it. The point is to exercise the extension the way an editor does —
// activation, the language client, the server over stdio — rather than to
// unit-test the handful of lines in extension.js.

const path = require("path");
const fs = require("fs");
const { runTests } = require("@vscode/test-electron");

async function main() {
  const extensionDevelopmentPath = path.resolve(__dirname, "..");
  const extensionTestsPath = path.resolve(__dirname, "suite", "index.js");
  const workspace = path.resolve(__dirname, "fixtures");

  // Point the extension at the binary built from this repository rather than
  // whatever happens to be on PATH, so the test covers the current code.
  const server = process.env.RSX_BINARY || resolveServerBinary();
  const settingsDir = path.join(workspace, ".vscode");
  fs.mkdirSync(settingsDir, { recursive: true });
  fs.writeFileSync(
    path.join(settingsDir, "settings.json"),
    JSON.stringify(
      {
        "rsx.server.path": server,
        // A GUI process does not inherit a login shell's PATH, so the
        // analyzer is named outright rather than looked up.
        "rsx.rustAnalyzer.path": resolveRustAnalyzer(),
      },
      null,
      2
    )
  );

  await runTests({
    extensionDevelopmentPath,
    extensionTestsPath,
    launchArgs: [
      workspace,
      "--disable-extensions",
      "--disable-gpu",
      // A fresh test instance treats the fixture folder as untrusted, which
      // blocks the setting that points at the server binary.
      "--disable-workspace-trust",
    ],
  });
}

function resolveServerBinary() {
  const repo = path.resolve(__dirname, "..", "..", "..");
  const candidates = [
    process.env.RSX_BINARY,
    path.join(repo, "target", "debug", "rsx"),
    path.join(repo, "target", "release", "rsx"),
    path.join(process.env.HOME || "", ".cargo", "bin", "rsx"),
  ].filter(Boolean);

  for (const candidate of candidates) {
    if (fs.existsSync(candidate)) {
      return candidate;
    }
  }
  throw new Error(
    `no rsx binary found; looked in:\n  ${candidates.join("\n  ")}\n` +
      "run `cargo build` first, or set RSX_BINARY"
  );
}

/// Finds rust-analyzer without relying on the PATH this process inherited.
function resolveRustAnalyzer() {
  if (process.env.RUST_ANALYZER) {
    return process.env.RUST_ANALYZER;
  }
  const candidates = [
    "/opt/homebrew/bin/rust-analyzer",
    "/usr/local/bin/rust-analyzer",
    path.join(process.env.HOME || "", ".cargo", "bin", "rust-analyzer"),
  ];
  return candidates.find((c) => fs.existsSync(c)) || "rust-analyzer";
}

main().catch((error) => {
  console.error("tests failed:", error);
  process.exit(1);
});
