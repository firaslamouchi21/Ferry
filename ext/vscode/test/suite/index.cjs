const assert = require("node:assert");
const vscode = require("vscode");

const EXPECTED_COMMANDS = [
  "ferry.open",
  "ferry.pair",
  "ferry.sendFile",
  "ferry.openInbox",
  "ferry.copyFingerprint",
  "ferry.refreshPeers",
  "ferry.startDaemon",
  "ferry.restartDaemon",
];

async function run() {
  const ext = vscode.extensions.getExtension("ferry.ferry-vscode");
  assert.ok(ext, "the Ferry extension was not found by id ferry.ferry-vscode");

  await ext.activate();
  assert.strictEqual(ext.isActive, true, "the extension did not activate");

  const all = new Set(await vscode.commands.getCommands(true));
  const missing = EXPECTED_COMMANDS.filter((c) => !all.has(c));
  assert.deepStrictEqual(missing, [], `commands not registered after activation: ${missing.join(", ")}`);

  // Opening the panel must create a webview and not throw, even with no daemon running.
  await vscode.commands.executeCommand("ferry.open");
  await new Promise((r) => setTimeout(r, 500));
  const tabs = vscode.window.tabGroups.all.flatMap((g) => g.tabs);
  const ferryTab = tabs.find((t) => (t.label || "").toLowerCase().includes("ferry"));
  assert.ok(ferryTab, "ferry.open did not open a Ferry webview tab");

  // Refresh must be harmless with no daemon.
  await vscode.commands.executeCommand("ferry.refreshPeers");

  console.log(`extension-host OK — activated, ${EXPECTED_COMMANDS.length} commands, webview opened`);
}

module.exports = { run };
