import { build, context } from "esbuild";
import { cpSync, mkdirSync, rmSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const root = path.dirname(fileURLToPath(import.meta.url));
const watch = process.argv.includes("--watch");

const options = {
  entryPoints: [path.join(root, "src/extension.ts")],
  bundle: true,
  outfile: path.join(root, "dist/extension.js"),
  platform: "node",
  format: "cjs",
  target: "node18",
  external: ["vscode"],
  sourcemap: true,
};

function copyUi() {
  const src = path.join(root, "../../ui/dist");
  const dest = path.join(root, "media/ui");
  rmSync(dest, { recursive: true, force: true });
  mkdirSync(dest, { recursive: true });
  cpSync(src, dest, { recursive: true });
}

copyUi();

if (watch) {
  const ctx = await context(options);
  await ctx.watch();
  console.log("esbuild watching");
} else {
  await build(options);
  console.log("extension bundled");
}
