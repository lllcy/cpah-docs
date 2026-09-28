use crate::converter::{ConversionArtifact, ConversionAsset};
use anydoc::model::{Block, CellSlot, Document, ImageSource, Inline};
use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

#[cfg(test)]
pub(crate) mod tests;

pub const MAX_INPUT_BYTES: u64 = 512 * 1024 * 1024;
const MAX_ASSET_BYTES: usize = 256 * 1024 * 1024;

#[derive(Debug)]
pub enum LocalConversion {
    Converted(ConversionArtifact),
    NeedsOcr { pages: Vec<u32>, page_count: u32 },
}

pub fn convert(source: &Path) -> Result<LocalConversion> {
    let file = File::open(source).context("无法读取待转换文件")?;
    if file.metadata()?.len() > MAX_INPUT_BYTES {
        bail!("原文件超过 512 MiB 安全上限");
    }
    let mut bytes = Vec::new();
    file.take(MAX_INPUT_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_INPUT_BYTES {
        bail!("原文件超过 512 MiB 安全上限");
    }
    let extension = source
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let artifact = match extension.as_str() {
        "txt" | "html" | "htm" => {
            let options = anytomd::ConversionOptions {
                extract_images: true,
                extract_comments: false,
                max_total_image_bytes: MAX_ASSET_BYTES,
                max_input_bytes: MAX_INPUT_BYTES as usize,
                max_uncompressed_zip_bytes: 2 * 1024 * 1024 * 1024,
                strict: false,
                image_describer: None,
            };
            let result = anytomd::convert_bytes(&bytes, &extension, &options)
                .context("anytomd 文本转换失败")?;
            ConversionArtifact {
                markdown: result.markdown,
                assets: result
                    .images
                    .into_iter()
                    .map(|(name, bytes)| ConversionAsset {
                        relative_path: PathBuf::from(name),
                        bytes,
                    })
                    .collect(),
                warnings: result
                    .warnings
                    .into_iter()
                    .map(|warning| warning.message)
                    .collect(),
            }
        }
        "pdf" => match anydoc::to_markdown_bytes(&bytes, anydoc::Format::Pdf) {
            Ok(markdown) => ConversionArtifact {
                markdown,
                assets: Vec::new(),
                warnings: vec![
                    "PDF 已在本地提取文字和表格；插图使用解析器原生占位，不导出图片附件。"
                        .to_string(),
                ],
            },
            Err(anydoc::ConvertError::NeedsOcr { pages, page_count }) => {
                return Ok(LocalConversion::NeedsOcr { pages, page_count });
            }
            Err(error) => return Err(error).context("anydoc PDF 本地转换失败"),
        },
        _ => {
            let format =
                anydoc::Format::from_extension(&extension).context("不支持的本地转换格式")?;
            let document = anydoc::to_document(&bytes, format).context("anydoc 文档解析失败")?;
            render_document(document)?
        }
    };
    if artifact.markdown.trim().is_empty() {
        bail!("本地转换未产生 Markdown 内容");
    }
    Ok(LocalConversion::Converted(artifact))
}

fn render_document(mut document: Document) -> Result<ConversionArtifact> {
    let mut assets = Vec::with_capacity(document.assets.len());
    let mut paths = HashMap::new();
    let mut total_bytes = 0_usize;
    for asset in std::mem::take(&mut document.assets) {
        total_bytes = total_bytes
            .checked_add(asset.bytes.len())
            .context("附件大小溢出")?;
        if total_bytes > MAX_ASSET_BYTES {
            bail!("本地附件合计超过 256 MiB 安全上限");
        }
        let extension = asset_extension(&asset.media_type, &asset.origin_part);
        let name = format!("asset-{:04}.{extension}", asset.id.0);
        if paths.insert(asset.id.0, name.clone()).is_some() {
            bail!("文档包含重复的附件 ID");
        }
        assets.push(ConversionAsset {
            relative_path: PathBuf::from(name),
            bytes: asset.bytes,
        });
    }
    let mut missing_images = 0;
    rewrite_blocks(&mut document.blocks, &paths, &mut missing_images, 0)?;
    for note in &mut document.notes {
        rewrite_blocks(&mut note.blocks, &paths, &mut missing_images, 0)?;
    }
    let warnings = if missing_images == 0 {
        Vec::new()
    } else {
        vec![format!(
            "{missing_images} 处图片缺少可读取的资源，仅保留替代文字。"
        )]
    };
    Ok(ConversionArtifact {
        markdown: anydoc::document_to_markdown(&document),
        assets,
        warnings,
    })
}

fn asset_extension<'a>(media_type: &str, origin: &'a str) -> &'a str {
    match media_type {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/bmp" | "image/x-ms-bmp" => "bmp",
        "image/tiff" => "tiff",
        "image/svg+xml" => "svg",
        "image/x-emf" | "image/emf" => "emf",
        "image/x-wmf" | "image/wmf" => "wmf",
        _ => Path::new(origin)
            .extension()
            .and_then(|value| value.to_str())
            .filter(|value| {
                !value.is_empty()
                    && value.len() <= 10
                    && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
            })
            .unwrap_or("bin"),
    }
}

fn rewrite_blocks(
    blocks: &mut [Block],
    paths: &HashMap<usize, String>,
    missing: &mut usize,
    depth: usize,
) -> Result<()> {
    if depth > 256 {
        bail!("文档结构嵌套超过安全上限");
    }
    for block in blocks {
        match block {
            Block::Paragraph(inlines)
            | Block::Heading {
                content: inlines, ..
            } => rewrite_inlines(inlines, paths, missing, depth + 1)?,
            Block::List(list) => {
                for item in &mut list.items {
                    rewrite_blocks(&mut item.blocks, paths, missing, depth + 1)?;
                }
            }
            Block::Table(table) => {
                for row in &mut table.grid {
                    for slot in row {
                        if let CellSlot::Origin(cell) = slot {
                            rewrite_blocks(&mut cell.blocks, paths, missing, depth + 1)?;
                        }
                    }
                }
            }
            Block::BlockQuote(children) => rewrite_blocks(children, paths, missing, depth + 1)?,
            Block::CodeBlock { .. } | Block::Rule | Block::Math(_) => {}
        }
    }
    Ok(())
}

fn rewrite_inlines(
    inlines: &mut [Inline],
    paths: &HashMap<usize, String>,
    missing: &mut usize,
    depth: usize,
) -> Result<()> {
    if depth > 256 {
        bail!("文档结构嵌套超过安全上限");
    }
    for inline in inlines {
        match inline {
            Inline::Image { source, .. } => match source {
                ImageSource::Asset(id) => {
                    if let Some(path) = paths.get(&id.0) {
                        // Upstream renders External references verbatim as Markdown URLs;
                        // relative attachment URLs deliberately use the same rendering path.
                        *source = ImageSource::External(path.clone());
                    } else {
                        *source = ImageSource::Unavailable;
                        *missing += 1;
                    }
                }
                ImageSource::Unavailable => *missing += 1,
                ImageSource::External(_) => {}
            },
            Inline::Link { content, .. } => rewrite_inlines(content, paths, missing, depth + 1)?,
            _ => {}
        }
    }
    Ok(())
}
