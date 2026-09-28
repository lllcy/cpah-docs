//! CPAH Docs adapter: retain worksheet drawing images beside the table.
//! Uses the package reader's path checks and resource limits and the shared
//! asset sink's deduplication; external references are never fetched.

use crate::error::ConvertError;
use crate::model::{Block, ImageSource, Inline};
use crate::package::relationships::{TargetMode, read_rels, rels_part_for};
use crate::package::xml::ns;
use crate::package::{Package, path};
use crate::shared::assets::{AssetSink, media_type_for};

const DRAWING_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing";

pub(super) fn read_images(
    pkg: &mut Package,
    sheet_part: &str,
    assets: &mut AssetSink,
) -> Result<Vec<Block>, ConvertError> {
    let rels = read_rels(pkg, &rels_part_for(sheet_part))?;
    let mut drawings: Vec<String> = rels
        .iter()
        .filter(|(_, rel)| rel.rel_type == DRAWING_REL && rel.mode == TargetMode::Internal)
        .filter_map(|(_, rel)| path::resolve(sheet_part, &rel.target).ok().map(|t| t.path))
        .collect();
    drawings.sort();
    drawings.dedup();
    let mut blocks = Vec::new();
    for drawing_part in drawings {
        let Some(drawing) = pkg.optional_xml_part(&drawing_part)? else {
            continue;
        };
        let image_rels = read_rels(pkg, &rels_part_for(&drawing_part))?;
        for blip in drawing.descendants(ns::A, "blip") {
            let source = match blip
                .attr_qualified(ns::R, "embed")
                .or_else(|| blip.attr_qualified(ns::R, "link"))
                .and_then(|id| image_rels.get(id))
            {
                Some(rel) if rel.mode == TargetMode::External && !rel.target.is_empty() => {
                    ImageSource::External(rel.target.clone())
                }
                Some(rel) if rel.mode == TargetMode::Internal => {
                    match path::resolve(&drawing_part, &rel.target) {
                        Ok(target) => match pkg.optional_part(&target.path)? {
                            Some(bytes) => ImageSource::Asset(assets.add(
                                media_type_for(&target.path),
                                target.path,
                                &bytes,
                            )?),
                            None => ImageSource::Unavailable,
                        },
                        Err(_) => ImageSource::Unavailable,
                    }
                }
                _ => ImageSource::Unavailable,
            };
            blocks.push(Block::Paragraph(vec![Inline::Image { alt: "Image".into(), source }]));
        }
    }
    Ok(blocks)
}
