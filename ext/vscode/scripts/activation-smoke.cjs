const Module = require("node:module");
const path = require("node:path");
const assert = require("node:assert");

const registered = { commands: new Set(), treeViews: new Set(), statusBars: 0 };
const disposable = { dispose() {} };

class EventEmitterStub {
  constructor() {
    this.listeners = new Set();
    this.event = (fn) => {
      this.listeners.add(fn);
      return { dispose: () => this.listeners.delete(fn) };
    };
  }
  fire(value) {
    for (const fn of this.listeners) fn(value);
  }
  dispose() {
    this.listeners.clear();
  }
}

class TreeItemStub {
  constructor(label, collapsibleState) {
    this.label = label;
    this.collapsibleState = collapsibleState;
  }
}

const vscodeStub = {
  StatusBarAlignment: { Left: 1, Right: 2 },
  ViewColumn: { Active: -1 },
  ProgressLocation: { Window: 10, Notification: 15 },
  EventEmitter: EventEmitterStub,
  TreeItem: TreeItemStub,
  TreeItemCollapsibleState: { None: 0, Collapsed: 1, Expanded: 2 },
  ThemeIcon: class {
    constructor(id) {
      this.id = id;
    }
  },
  Uri: {
    joinPath: (base, ...parts) => ({ fsPath: path.join(base.fsPath, ...parts), path: path.join(base.fsPath, ...parts) }),
    file: (p) => ({ fsPath: p, path: p }),
  },
  window: {
    createStatusBarItem: () => {
      registered.statusBars += 1;
      return { text: "", command: "", show() {}, hide() {}, dispose() {} };
    },
    registerTreeDataProvider: (id) => {
      registered.treeViews.add(id);
      return disposable;
    },
    createWebviewPanel: () => ({
      webview: { asWebviewUri: (u) => u, postMessage() {}, onDidReceiveMessage() {}, cspSource: "vscode-webview:", html: "" },
      reveal() {},
      onDidDispose() {},
      dispose() {},
    }),
    showInformationMessage: () => Promise.resolve(),
    showWarningMessage: () => Promise.resolve(),
    showErrorMessage: () => Promise.resolve(),
    withProgress: (_opts, task) => task({ report() {} }),
    activeTextEditor: undefined,
  },
  commands: {
    registerCommand: (id) => {
      registered.commands.add(id);
      return disposable;
    },
    executeCommand: () => Promise.resolve(),
  },
  workspace: {
    getConfiguration: () => ({
      get: (key) => (key === "daemonPath" ? "/nonexistent/ferry-daemon-smoke" : undefined),
    }),
  },
  env: { clipboard: { writeText: () => Promise.resolve() } },
};

const originalLoad = Module._load;
Module._load = function patched(request, parent, isMain) {
  if (request === "vscode") return vscodeStub;
  return originalLoad.call(this, request, parent, isMain);
};

const distPath = path.resolve(__dirname, "..", "dist", "extension.js");
const ext = require(distPath);

const context = {
  subscriptions: [],
  extensionUri: { fsPath: path.resolve(__dirname, ".."), path: path.resolve(__dirname, "..") },
  extensionPath: path.resolve(__dirname, ".."),
  globalStorageUri: { fsPath: require("node:os").tmpdir(), path: require("node:os").tmpdir() },
};

assert.strictEqual(typeof ext.activate, "function", "extension must export activate()");

Promise.resolve(ext.activate(context)).then(() => {
  const expectedCommands = require("../package.json").contributes.commands.map((c) => c.command);
  const missing = expectedCommands.filter((c) => !registered.commands.has(c));
  assert.deepStrictEqual(missing, [], `activate() did not register: ${missing.join(", ")}`);
  assert.ok(registered.treeViews.has("ferryPeers"), "the ferryPeers tree view was not registered");
  assert.ok(registered.statusBars >= 1, "no status bar item was created");
  assert.ok(context.subscriptions.length >= expectedCommands.length, "disposables were not tracked on the context");

  if (typeof ext.deactivate === "function") ext.deactivate();

  console.log(
    `activation smoke OK — ${registered.commands.size} commands, tree view + status bar registered, no throw`,
  );
  process.exit(0);
}).catch((err) => {
  console.error("activation smoke FAILED:", err);
  process.exit(1);
});
