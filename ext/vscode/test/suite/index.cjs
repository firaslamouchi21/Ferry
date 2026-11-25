const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");
const vscode = require("vscode");

const EXPECTED_COMMANDS = [
  "ferry.open",
  "ferry.pair",
  "ferry.sendFile",
  "ferry.openInbox",
  "ferry.copyFingerprint",
  "ferry.startDaemon",
  "ferry.stopDaemon",
  "ferry.restartDaemon",
];

async function run() {
  const ext = vscode.extensions.getExtension("ferry.ferry-p2p");
  assert.ok(ext, "the Ferry extension was not found by id ferry.ferry-p2p");

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

  if (process.env.FERRY_TEST_DAEMON && process.platform === "linux") await driveRealDaemon();

  console.log(`extension-host OK — activated, ${EXPECTED_COMMANDS.length} commands, editor panel + sidebar webviews both mounted`);
}

async function waitFor(what, check) {
  for (let i = 0; i < 100; i += 1) {
    if (await check()) return;
    await new Promise((r) => setTimeout(r, 100));
  }
  throw new Error(`timed out waiting for ${what}`);
}

async function driveRealDaemon() {
  const socket = path.join(process.env.XDG_DATA_HOME, "ferry", "ferry.sock");
  await vscode.commands.executeCommand("ferry.startDaemon");
  await waitFor("the daemon socket", () => fs.existsSync(socket));
  await vscode.env.clipboard.writeText("");
  await vscode.commands.executeCommand("ferry.copyFingerprint");
  const first = await vscode.env.clipboard.readText();
  assert.match(first, /^[0-9a-f]{16}/, "copyFingerprint did not reach the running daemon");

  await vscode.commands.executeCommand("ferry.restartDaemon");
  await vscode.env.clipboard.writeText("");
  await vscode.commands.executeCommand("ferry.copyFingerprint");
  assert.strictEqual(await vscode.env.clipboard.readText(), first, "restarting the daemon changed its identity");

  await vscode.commands.executeCommand("ferry.stopDaemon");
  await waitFor("the daemon to stop", () => !fs.existsSync(socket));
  console.log("extension-host OK — started, queried, restarted, and stopped a real daemon");
}

module.exports = { run };
