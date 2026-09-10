import { execFileSync } from "node:child_process";
import { chmodSync, copyFileSync, existsSync, mkdirSync, rmSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const args = process.argv.slice(2);
const opt = (name) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 ? args[i + 1] : undefined;
};

const target = opt("target");
const daemon = opt("daemon") ?? process.env.FERRY_DAEMON_BIN;
const out = opt("out") ?? (target ? `ferry-${target}.vsix` : "ferry.vsix");

const binDir = path.join(root, "bin");
rmSync(binDir, { recursive: true, force: true });

if (daemon) {
  if (!existsSync(daemon)) {
    console.error(`--daemon path does not exist: ${daemon}`);
    process.exit(1);
  }
  mkdirSync(binDir, { recursive: true });
  const isWin = target?.startsWith("win32") || daemon.endsWith(".exe");
  const dest = path.join(binDir, isWin ? "ferry-daemon.exe" : "ferry-daemon");
  copyFileSync(daemon, dest);
  if (!isWin) chmodSync(dest, 0o755);
  console.log(`bundled ${daemon} -> ${dest}`);
} else {
  console.log("no --daemon given: packaging a generic .vsix (extension falls back to ferry.daemonPath / PATH)");
}

const run = (cmd, cmdArgs) =>
  execFileSync(cmd, cmdArgs, { cwd: root, stdio: "inherit" });

run("pnpm", ["run", "build"]);
const vsceArgs = ["package", "--no-dependencies", "-o", out];
if (target) vsceArgs.push("--target", target);
run("pnpm", ["exec", "vsce", ...vsceArgs]);

rmSync(binDir, { recursive: true, force: true });
console.log(`packaged ${out}`);
