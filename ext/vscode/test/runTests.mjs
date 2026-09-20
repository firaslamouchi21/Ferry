import { fileURLToPath } from "node:url";
import path from "node:path";
import { runTests } from "@vscode/test-electron";

const here = path.dirname(fileURLToPath(import.meta.url));
const extensionRoot = path.resolve(here, "..");
const cacheRoot = path.join(extensionRoot, ".vscode-test");
const quoteOnWindows = (p) => (process.platform === "win32" ? `"${p}"` : p);
delete process.env.ELECTRON_RUN_AS_NODE;

try {
  await runTests({
    extensionDevelopmentPath: quoteOnWindows(extensionRoot),
    extensionTestsPath: quoteOnWindows(path.resolve(here, "suite/index.cjs")),
    launchArgs: [
      "--disable-extensions",
      "--disable-gpu",
      `--extensions-dir=${quoteOnWindows(path.join(cacheRoot, "extensions"))}`,
      `--user-data-dir=${quoteOnWindows(path.join(cacheRoot, "user-data"))}`,
    ],
  });
} catch (err) {
  console.error("extension-host tests failed:", err);
  process.exit(1);
}
