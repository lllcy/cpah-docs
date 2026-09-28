# Vendored anydoc

- Upstream: https://github.com/firecrawl/anydoc
- Version: 0.2.4, from https://static.crates.io/crates/anydoc/anydoc-0.2.4.crate
- Archive SHA-256: `cf0d78e4cfe3654eb3ea04422ec5a94352000ee56e97902125e68e51f7527062`
- License: MIT; the original `LICENSE` is included.

Local patches:

- `src/lib.rs` publicly re-exports `document_to_markdown`, with an API comment.
  CPAH Docs resolves embedded image references to local attachments before rendering.
- `src/formats/sheet/images.rs`, called by the XLSX/XLSM and XLSB readers, retains
  worksheet drawing images after each table. Upstream 0.2.4 omits these images.
  This reuses upstream package limits, path resolution, and asset deduplication;
  external image URLs are preserved without fetching them. BIFF XLS is unchanged.
- `rustfmt.toml` is restored from the upstream v0.2.4 tag (the published crate
  excludes it) to retain upstream formatting in repository checks.

Source from the published crate is used rather than the moving main branch.
When updating, review these patches and run the application's document and
attachment regression tests.
