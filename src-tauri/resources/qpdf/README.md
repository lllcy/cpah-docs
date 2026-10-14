# Embedded QPDF runtime

`npm run prepare:qpdf` downloads the official QPDF 12.4.2 binaries and checks
the SHA256 values pinned in `manifest.json`. It retains only the executable,
runtime libraries and notices, then creates reproducible compressed ZIPs.
Downloads and generated ZIPs are ignored by Git. Cached verified downloads
allow subsequent offline builds. macOS preparation includes both architectures
for the universal app; a particular target can also be requested explicitly:

```shell
node scripts/prepare-qpdf.mjs windows-x64
node scripts/prepare-qpdf.mjs macos-x64 macos-arm64
```

Tauri dev/build and release scripts prepare the resources automatically.
Direct Cargo builds require the preparation step first. Cargo does not download
or invoke network tools. `build.rs` embeds only the matching target's ZIP.
The application verifies extracted cache files and repairs an incomplete or
damaged runtime before using it. QPDF runs as a hidden child process on Windows,
has a five-minute per-operation timeout and responds to profile cancellation.

PDFs above 64 MiB are split before local extraction. Local chunks target 32 MiB
and 50 pages. Single pages may be larger, up to the existing 190 MB safeguard.
MinerU physical chunks retain their 180 MB target and 200-page maximum. Source
PDFs remain capped at 1 GiB. This removes full-source byte loading and document
cloning for large files, but does not impose constant memory use on QPDF or on
the text parser. Their object and single-page complexity still matter.

Official source: https://github.com/qpdf/qpdf/releases/tag/v12.4.2
Dependency notices and license texts are included with each embedded payload
and in the app's generated `THIRD_PARTY_LICENSES.md`.
