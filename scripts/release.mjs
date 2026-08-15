import { spawnSync } from "node:child_process";

const releaseCommand = process.platform === "win32"
  ? {
      command: "powershell.exe",
      args: ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "scripts/release.ps1"],
    }
  : process.platform === "darwin"
    ? { command: "/bin/bash", args: ["scripts/release.sh"] }
    : null;

if (!releaseCommand) {
  console.error(`Unsupported release platform: ${process.platform}`);
  process.exit(1);
}

const result = spawnSync(releaseCommand.command, releaseCommand.args, { stdio: "inherit" });
if (result.error) throw result.error;
process.exit(result.status ?? 1);
