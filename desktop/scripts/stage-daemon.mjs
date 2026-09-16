import { execFileSync } from "node:child_process";
import { chmodSync, copyFileSync, existsSync, mkdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(here, "..", "..");
const target = process.env.FERRY_TARGET ?? execFileSync("rustc", ["-vV"]).toString().match(/^host: (.+)$/m)[1];
const exe = target.includes("windows") ? ".exe" : "";
const profileArg = process.argv.indexOf("--profile");
const profile = profileArg !== -1 ? process.argv[profileArg + 1] : process.env.FERRY_PROFILE ?? "release";

const candidates = [
  process.env.FERRY_DAEMON_BIN,
  path.join(repo, "target", target, profile, `ferry-daemon${exe}`),
  path.join(repo, "target", profile, `ferry-daemon${exe}`),
].filter(Boolean);
const source = candidates.find((c) => existsSync(c));
if (!source) {
  console.error(`stage-daemon: no ferry-daemon${exe} found — build it first:\n  cargo build --${profile} -p ferry-daemon${process.env.FERRY_TARGET ? ` --target ${target}` : ""}\nlooked in:\n  ${candidates.join("\n  ")}`);
  process.exit(1);
}

const dir = path.join(here, "..", "src-tauri", "binaries");
mkdirSync(dir, { recursive: true });
const dest = path.join(dir, `ferry-daemon-${target}${exe}`);
copyFileSync(source, dest);
if (!exe) chmodSync(dest, 0o755);
console.log(`stage-daemon: ${source} -> ${dest}`);
