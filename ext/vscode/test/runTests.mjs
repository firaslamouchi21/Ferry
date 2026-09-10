import { fileURLToPath } from "node:url";
import path from "node:path";
import { runTests } from "@vscode/test-electron";

const here = path.dirname(fileURLToPath(import.meta.url));

try {
  await runTests({
    extensionDevelopmentPath: path.resolve(here, ".."),
    extensionTestsPath: path.resolve(here, "suite/index.cjs"),
    launchArgs: ["--disable-extensions", "--disable-gpu"],
  });
} catch (err) {
  console.error("extension-host tests failed:", err);
  process.exit(1);
}
