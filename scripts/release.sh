#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$project_root"

package_version="$(node -e 'const fs = require("node:fs"); process.stdout.write(JSON.parse(fs.readFileSync("package.json", "utf8")).version)')"
tauri_version="$(node -e 'const fs = require("node:fs"); process.stdout.write(JSON.parse(fs.readFileSync("src-tauri/tauri.conf.json", "utf8")).version)')"
cargo_version="$(sed -nE '/^\[package\]/,/^\[/{s/^version[[:space:]]*=[[:space:]]*"([^"]+)"/\1/p;}' src-tauri/Cargo.toml | head -n 1)"

if [[ "$package_version" != "$cargo_version" || "$package_version" != "$tauri_version" ]]; then
  echo "Version mismatch: package.json=$package_version, Cargo.toml=$cargo_version, tauri.conf.json=$tauri_version" >&2
  exit 1
fi

rustup target add aarch64-apple-darwin x86_64-apple-darwin
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
npm run build
cargo test --manifest-path src-tauri/Cargo.toml --all-targets
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
# The DMG builder otherwise asks Finder to arrange the bundle window. That
# AppleScript step is unnecessary for releases and can hang on headless Macs.
CI=true npx tauri build --target universal-apple-darwin --bundles dmg

dmg_directory="src-tauri/target/universal-apple-darwin/release/bundle/dmg"
mapfile_supported=false
if command -v mapfile >/dev/null 2>&1; then
  mapfile_supported=true
fi
if [[ "$mapfile_supported" == true ]]; then
  mapfile -t dmg_files < <(find "$dmg_directory" -maxdepth 1 -type f -name '*.dmg' -print)
else
  dmg_files=()
  while IFS= read -r dmg_file; do
    dmg_files+=("$dmg_file")
  done < <(find "$dmg_directory" -maxdepth 1 -type f -name '*.dmg' -print)
fi
if [[ "${#dmg_files[@]}" -ne 1 ]]; then
  echo "Expected exactly one release DMG in $dmg_directory, found ${#dmg_files[@]}" >&2
  exit 1
fi

artifact_directory="release"
artifact_name="CPAH-Docs-v${package_version}-macos-universal.dmg"
mkdir -p "$artifact_directory"
cp "${dmg_files[0]}" "$artifact_directory/$artifact_name"
shasum -a 256 "$artifact_directory/$artifact_name" | sed "s#${artifact_directory}/##" > "$artifact_directory/SHA256SUMS.txt"

echo "Release artifact: $project_root/$artifact_directory/$artifact_name"
echo "SHA-256: $(cut -d ' ' -f 1 "$artifact_directory/SHA256SUMS.txt")"
