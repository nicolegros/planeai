import { spawnSync, execFileSync } from "node:child_process";
import { chmodSync, copyFileSync, existsSync, mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { platform } from "node:process";

const root = process.cwd();
const tauriDir = join(root, "src-tauri");
const args = process.argv.slice(2);
const runTauri = args[0] === "--run-tauri";
const tauriArgs = runTauri ? args.slice(1) : [];
const requestedTarget = targetFromArgs(tauriArgs);
const configuredTarget = process.env.TAURI_ENV_TARGET_TRIPLE;
const target =
  requestedTarget ??
  configuredTarget ??
  execFileSync("rustc", ["--print", "host-tuple"], { cwd: root, encoding: "utf8" }).trim();
const debugTargetArgs = requestedTarget || configuredTarget ? ["--target", target] : [];
const extension = target.includes("windows") ? ".exe" : "";
const releaseDir = join(tauriDir, "target", target, "release");
const binariesDir = join(tauriDir, "binaries");
const sidecars = [
  ["planeai-cli-bin", "planeai-cli"],
  ["planeai-daemon-bin", "planeai-daemon"],
  ["planeai-plugin-jira", "planeai-plugin-jira"],
];

// Tauri validates every externalBin while compiling the main app, including
// while a sidecar's dependency graph happens to compile that app first.
mkdirSync(binariesDir, { recursive: true });
for (const [, binaryName] of sidecars) {
  const placeholder = join(binariesDir, `${binaryName}-${target}${extension}`);
  if (!existsSync(placeholder)) writeFileSync(placeholder, "");
}

for (const [packageName] of sidecars) {
  run("cargo", ["clean", "--release", "--target", target, "-p", packageName], tauriDir);
  run("cargo", ["build", "--release", "--target", target, "-p", packageName], tauriDir);
}

for (const [, binaryName] of sidecars) {
  const source = join(releaseDir, `${binaryName}${extension}`);
  const destination = join(binariesDir, `${binaryName}-${target}${extension}`);
  if (!existsSync(source)) {
    throw new Error(`sidecar build did not produce ${source}`);
  }
  copyFileSync(source, destination);
  if (platform !== "win32") chmodSync(destination, 0o755);
}

// Tauri debug builds resolve the trusted plugin sidecar beside the debug host.
run("cargo", ["build", ...debugTargetArgs, "-p", "planeai-plugin-jira"], tauriDir);

if (runTauri) run("pnpm", ["exec", "tauri", ...tauriArgs], root);

function targetFromArgs(args) {
  const targetIndex = args.indexOf("--target");
  if (targetIndex >= 0 && args[targetIndex + 1]) return args[targetIndex + 1];
  const targetArgument = args.find((argument) => argument.startsWith("--target="));
  return targetArgument?.slice("--target=".length);
}

function run(command, args, cwd) {
  const result = spawnSync(command, args, { cwd, stdio: "inherit" });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`${command} ${args.join(" ")} failed with status ${result.status}`);
  }
}
