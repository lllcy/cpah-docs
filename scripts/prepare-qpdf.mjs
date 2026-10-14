import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { readZip, writeZip } from "./qpdf-archive.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const resources = join(root, "src-tauri/resources/qpdf");
const manifest = JSON.parse(readFileSync(join(resources, "manifest.json"), "utf8"));
const platform = { win32: "windows", darwin: "macos", linux: "linux" }[process.platform];
const requested = process.argv.slice(2);
const targets = requested.includes("--all") ? Object.keys(manifest.targets)
  : requested.length ? requested : platform === "macos" ? ["macos-x64", "macos-arm64"] : [`${platform}-${process.arch}`];
const digest = bytes => createHash("sha256").update(bytes).digest("hex");
mkdirSync(join(resources, "downloads"), { recursive: true });
for (const target of targets) {
  const specification = manifest.targets[target];
  if (!specification) throw new Error(`Unsupported QPDF target: ${target}`);
  const output = join(resources, `qpdf-${target}.zip`);
  const cached = join(resources, "downloads", specification.asset);
  if (!existsSync(cached) || digest(readFileSync(cached)) !== specification.sha256) {
    console.log(`Downloading verified QPDF ${manifest.version} for ${target}...`);
    const url = `https://github.com/qpdf/qpdf/releases/download/v${manifest.version}/${specification.asset}`;
    let bytes;
    try {
      const response = await fetch(url, { signal: AbortSignal.timeout(180000) });
      if (!response.ok) throw new Error(`QPDF download failed: HTTP ${response.status}`);
      bytes = Buffer.from(await response.arrayBuffer());
    } catch (error) {
      // curl also supports proxy configurations that Node fetch may not inherit.
      const result = spawnSync(process.platform === "win32" ? "curl.exe" : "curl", [
        "--fail", "--location", "--silent", "--show-error", "--retry", "2",
        "--connect-timeout", "20", "--max-time", "180", "--output", `${cached}.tmp`, url,
      ], { stdio: "inherit" });
      if (result.error || result.status !== 0) throw new Error("QPDF download failed", { cause: error });
      bytes = readFileSync(`${cached}.tmp`);
    }
    if (digest(bytes) !== specification.sha256) throw new Error("QPDF download SHA256 mismatch");
    writeFileSync(`${cached}.tmp`, bytes);
    renameSync(`${cached}.tmp`, cached);
  }
  const archive = readZip(readFileSync(cached));
  const executable = [...archive.keys()].find(name => /(^|\/)bin\/qpdf(?:\.exe)?$/.test(name));
  if (!executable) throw new Error("Official QPDF executable missing");
  const prefix = executable.slice(0, executable.lastIndexOf("bin/"));
  const files = new Map();
  for (const [name, bytes] of archive) {
    if (!name.startsWith(prefix)) continue;
    const relative = name.slice(prefix.length);
    if (/^bin\/qpdf(?:\.exe)?$/.test(relative) || /^bin\/[^/]+\.dll$/i.test(relative) || /^lib\/[^/]+(?:\.dylib|\.so(?:\.\d+)*)$/.test(relative)) files.set(relative, bytes);
  }
  for (const name of ["LICENSE-QPDF.txt", "LICENSE-MSVC.txt", "THIRD_PARTY_NOTICES.md"]) files.set(name, readFileSync(join(resources, name)));
  const payload = writeZip(files);
  if (!existsSync(output) || digest(readFileSync(output)) !== digest(payload)) {
    writeFileSync(`${output}.tmp`, payload);
    renameSync(`${output}.tmp`, output);
  }
  console.log(`QPDF ${target}: ${files.size} runtime files, ${(payload.length / 1048576).toFixed(2)} MiB embedded payload`);
}
