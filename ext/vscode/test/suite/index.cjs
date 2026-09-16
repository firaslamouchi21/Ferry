const assert = require("node:assert");
const vscode = require("vscode");

const EXPECTED_COMMANDS = [
  "ferry.open",
  "ferry.pair",
  "ferry.sendFile",
  "ferry.openInbox",
  "ferry.copyFingerprint",
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

  const api = ext.exports;
  assert.ok(api && api.onWebviewMessage, "activate() did not return the extension API");
  const started = new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("the webview opened but the UI never mounted (no 'start' message within 15s)")), 15000);
    const sub = api.onWebviewMessage((message) => {
      console.log("webview ->", JSON.stringify(message));
      if (message && message.kind === "start") {
        clearTimeout(timer);
        sub.dispose();
        resolve();
      }
    });
  });
  await vscode.commands.executeCommand("ferry.open");
  await started;
  const tabs = vscode.window.tabGroups.all.flatMap((g) => g.tabs);
  const ferryTab = tabs.find((t) => (t.label || "").toLowerCase().includes("ferry"));
  assert.ok(ferryTab, "ferry.open did not open a Ferry webview tab");

  const sidebarStarted = new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("the sidebar webview view never mounted (no 'start' message within 15s)")), 15000);
    const sub = api.onWebviewMessage((message) => {
      if (message && message.kind === "start") {
        clearTimeout(timer);
        sub.dispose();
        resolve();
      }
    });
  });
  await vscode.commands.executeCommand("ferryMain.focus");
  await sidebarStarted;

  console.log(`extension-host OK — activated, ${EXPECTED_COMMANDS.length} commands, editor panel + sidebar webviews both mounted`);
}

module.exports = { run };
