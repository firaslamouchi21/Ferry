import { fileURLToPath } from "node:url";
import path from "node:path";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { runTests } from "@vscode/test-electron";

const here = path.dirname(fileURLToPath(import.meta.url));
const extensionRoot = path.resolve(here, "..");
const cacheRoot = path.join(extensionRoot, ".vscode-test");
const quoteOnWindows = (p) => (process.platform === "win32" ? `"${p}"` : p);
delete process.env.ELECTRON_RUN_AS_NODE;

const userData = path.join(cacheRoot, "user-data");
const dataHome = mkdtempSync(path.join(tmpdir(), "ferry-vscode-e2e-"));
const daemon = process.env.FERRY_TEST_DAEMON;
mkdirSync(path.join(userData, "User"), { recursive: true });
writeFileSync(
  path.join(userData, "User", "settings.json"),
  JSON.stringify({
    "ferry.autoStartDaemon": false,
    ...(daemon ? { "ferry.daemonPath": path.resolve(daemon) } : {}),
  }),
);
mkdirSync(path.join(dataHome, "ferry"), { recursive: true });
writeFileSync(path.join(dataHome, "ferry", "config.toml"), 'listen_port = 47997\nidentity_keystore = "file"\n');

try {
  await runTests({
    extensionDevelopmentPath: quoteOnWindows(extensionRoot),
    extensionTestsEnv: {
      XDG_DATA_HOME: dataHome,
      FERRY_IDENTITY_PASSPHRASE: "extension end-to-end test only",
      ...(daemon ? { FERRY_TEST_DAEMON: daemon } : {}),
    },
    extensionTestsPath: quoteOnWindows(path.resolve(here, "suite/index.cjs")),
    launchArgs: [
      "--disable-extensions",
      "--disable-gpu",
      `--extensions-dir=${quoteOnWindows(path.join(cacheRoot, "extensions"))}`,
      `--user-data-dir=${quoteOnWindows(userData)}`,
    ],
  });
} catch (err) {
  console.error("extension-host tests failed:", err);
  process.exitCode = 1;
} finally {
  rmSync(dataHome, { recursive: true, force: true });
}
