import { spawnSync } from "node:child_process";

const script = process.platform === "win32"
  ? "release:windows"
  : process.platform === "darwin"
    ? "release:macos"
    : null;

if (!script) {
  console.error(`Unsupported release platform: ${process.platform}`);
  process.exit(1);
}

const npm = process.platform === "win32" ? "npm.cmd" : "npm";
const result = spawnSync(npm, ["run", script], { stdio: "inherit" });
if (result.error) throw result.error;
process.exit(result.status ?? 1);
