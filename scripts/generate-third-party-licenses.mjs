import { spawnSync } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import { existsSync, readdirSync, readFileSync, unlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const projectRoot = resolve(scriptDirectory, "..");
const cargoAboutArgument = process.argv.indexOf("--cargo-about");
const cargoAbout = cargoAboutArgument >= 0
  ? process.argv[cargoAboutArgument + 1]
  : "cargo-about";

if (!cargoAbout) {
  throw new Error("--cargo-about requires an executable path");
}

const rustOutput = join(tmpdir(), `cpah-docs-rust-licenses-${randomUUID()}.md`);
const packageJson = JSON.parse(readFileSync(join(projectRoot, "package.json"), "utf8"));

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: projectRoot,
    encoding: "utf8",
    ...options,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    if (result.stdout) process.stdout.write(result.stdout);
    if (result.stderr) process.stderr.write(result.stderr);
    throw new Error(`${command} exited with status ${result.status}`);
  }
  return result.stdout ?? "";
}

function compareText(left, right) {
  return left < right ? -1 : left > right ? 1 : 0;
}

function htmlEncode(value) {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

try {
  run(cargoAbout, [
    "generate",
    "--manifest-path", "src-tauri/Cargo.toml",
    "--config", "about.toml",
    "--locked",
    "--fail",
    "--output-file", rustOutput,
    "about.hbs",
  ]);

  const npmCli = [
    process.env.npm_execpath,
    join(dirname(process.execPath), "node_modules", "npm", "bin", "npm-cli.js"),
    resolve(dirname(process.execPath), "..", "lib", "node_modules", "npm", "bin", "npm-cli.js"),
  ].find((candidate) => candidate && existsSync(candidate));
  if (!npmCli) throw new Error("Unable to locate npm-cli.js");
  const packages = JSON.parse(run(process.execPath, [npmCli, "query", ".prod"]))
    .filter((item) => item.location)
    .sort((left, right) =>
      compareText(left.name ?? "", right.name ?? "")
      || compareText(left.version ?? "", right.version ?? ""));

  const npmLines = [
    "## npm 运行时依赖",
    "",
    "依赖名称和许可证来自锁定的 npm 依赖树；相同的许可证正文只收录一次。",
    "",
  ];
  const licenseTexts = new Map();
  for (const dependency of packages) {
    const packageKey = `${dependency.name}@${dependency.version}`;
    npmLines.push(`- \`${packageKey}\` — ${dependency.license || "未声明"}`);

    const licenseFiles = readdirSync(dependency.path, { withFileTypes: true })
      .filter((entry) => entry.isFile()
        && /^(LICENSE|LICENCE|COPYING|NOTICE)([._-].*)?$/i.test(entry.name))
      .map((entry) => entry.name)
      .sort(compareText);
    for (const licenseFile of licenseFiles) {
      const text = readFileSync(join(dependency.path, licenseFile), "utf8")
        .replace(/\r\n?/g, "\n")
        .trim();
      if (!text) continue;
      const hash = createHash("sha256").update(text, "utf8").digest("hex");
      const entry = licenseTexts.get(hash) ?? { text, packages: new Set() };
      entry.packages.add(packageKey);
      licenseTexts.set(hash, entry);
    }
  }

  const sortedLicenseTexts = [...licenseTexts.values()].sort((left, right) =>
    compareText([...left.packages][0], [...right.packages][0]));
  for (const entry of sortedLicenseTexts) {
    const names = [...entry.packages].join(", ");
    npmLines.push(
      "",
      "<details>",
      `<summary>${htmlEncode(names)}</summary>`,
      "",
      `<pre>${htmlEncode(entry.text)}</pre>`,
      "</details>",
    );
  }

  const content = [
    "# 第三方软件许可与声明",
    "",
    `本清单适用于 CPAH Docs v${packageJson.version}。CPAH Docs 的原创代码以 MIT License 授权；下列第三方组件继续适用其各自的许可证。`,
    "",
    "本清单根据本版本锁定的依赖自动生成。组件名称后的 crates.io 精确版本页面提供对应源码包；其中 MPL-2.0 组件的源码继续以 MPL-2.0 提供。",
    "",
    readFileSync(rustOutput, "utf8").replace(/\r\n?/g, "\n").trimEnd(),
    "",
    ...npmLines,
  ].join("\n").replace(/[ \t]+$/gm, "").trimEnd() + "\n";
  writeFileSync(join(projectRoot, "THIRD_PARTY_LICENSES.md"), content, "utf8");
} finally {
  try {
    unlinkSync(rustOutput);
  } catch (error) {
    if (error?.code !== "ENOENT") throw error;
  }
}
