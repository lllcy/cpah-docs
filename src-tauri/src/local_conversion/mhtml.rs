use super::{MAX_ASSET_BYTES, asset_extension, convert_text};
use crate::converter::{ConversionArtifact, ConversionAsset};
use anyhow::{Context, Result, bail};
use encoding_rs::{Encoding, UTF_8};
use mailparse::{MailHeaderMap, ParsedMail};
use percent_encoding::percent_decode_str;
use reqwest::Url;
use scraper::{Html, Node};
use std::collections::HashMap;
use std::path::PathBuf;

const MAX_PARTS: usize = 10_000;

pub(super) fn convert(bytes: &[u8]) -> Result<ConversionArtifact> {
    convert_with_asset_limit(bytes, MAX_ASSET_BYTES)
}

fn convert_with_asset_limit(bytes: &[u8], asset_limit: usize) -> Result<ConversionArtifact> {
    let archive = mailparse::parse_mail(bytes).context("MHTML 存档解析失败")?;
    let mut parts = Vec::new();
    collect_parts(&archive, None, &mut parts)?;
    let root = html_part(&archive)?.context("MHTML 存档中没有 HTML 正文")?;
    let root_location = parts
        .iter()
        .find(|(part, _)| std::ptr::eq(*part, root))
        .and_then(|(_, location)| location.as_ref());
    let mut document = Html::parse_document(&decode_html(root)?);
    let base = document
        .tree
        .nodes()
        .find_map(|node| {
            let element = node.value().as_element()?;
            (element.name() == "base")
                .then(|| element.attr("href"))
                .flatten()
                .and_then(|href| resolve_url(href, root_location))
        })
        .or_else(|| root_location.cloned());

    // Map archive identifiers to MIME parts; decode only resources actually used
    // by the selected page. Archive filenames never become output paths.
    let mut resources = HashMap::new();
    for (index, (part, location)) in parts.iter().enumerate() {
        if !part.ctype.mimetype.starts_with("image/") {
            continue;
        }
        if let Some(id) = part.headers.get_first_value("Content-ID") {
            resources
                .entry(format!("cid:{}", content_id(&id)))
                .or_insert(index);
        }
        if let Some(value) = part.headers.get_first_value("Content-Location") {
            resources
                .entry(reference_key(&value, base.as_ref()))
                .or_insert(index);
            if let Some(location) = location {
                resources
                    .entry(reference_key(location.as_str(), None))
                    .or_insert(index);
            }
        }
    }

    let node_ids = document
        .tree
        .nodes()
        .filter(|node| node.value().is_element())
        .map(|node| node.id())
        .collect::<Vec<_>>();
    let mut exported = HashMap::<usize, String>::new();
    let mut assets = Vec::new();
    let mut total_bytes = 0_usize;
    let mut missing_images = false;
    for id in node_ids {
        let mut node = document
            .tree
            .get_mut(id)
            .context("MHTML 正文节点不可读取")?;
        let Node::Element(element) = node.value() else {
            continue;
        };
        let attribute = match element.name() {
            "img" => "src",
            "a" => "href",
            _ => continue,
        };
        for (name, value) in element.attrs.iter_mut() {
            if name.local.as_ref() != attribute {
                continue;
            }
            let original: &str = value.as_ref();
            let key = reference_key(original, base.as_ref());
            let replacement = if let Some(&index) = resources.get(&key) {
                if let Some(path) = exported.get(&index) {
                    path.clone()
                } else {
                    let part = parts[index].0;
                    let bytes = part.get_body_raw().context("MHTML 图片解码失败")?;
                    total_bytes = total_bytes
                        .checked_add(bytes.len())
                        .context("附件大小溢出")?;
                    if total_bytes > asset_limit {
                        bail!("本地附件合计超过 256 MiB 安全上限");
                    }
                    let extension = asset_extension(&part.ctype.mimetype, "");
                    let path = format!("mhtml-{:04}.{extension}", assets.len());
                    assets.push(ConversionAsset {
                        relative_path: PathBuf::from(&path),
                        bytes,
                    });
                    exported.insert(index, path.clone());
                    path
                }
            } else {
                if attribute == "src" && key.starts_with("cid:") {
                    missing_images = true;
                }
                if original.starts_with('#') {
                    original.to_string()
                } else {
                    resolve_url(original, base.as_ref())
                        .map(|url| url.to_string())
                        .unwrap_or_else(|| original.to_string())
                }
            };
            *value = replacement.as_str().into();
        }
    }
    let mut artifact = convert_text(document.html().as_bytes(), "html")?;
    artifact.assets = assets;
    if missing_images {
        artifact
            .warnings
            .push("MHTML 中部分图片资源缺失，仅保留原始引用。".to_string());
    }
    Ok(artifact)
}

fn collect_parts<'a, 'b>(
    part: &'a ParsedMail<'b>,
    inherited_location: Option<&Url>,
    parts: &mut Vec<(&'a ParsedMail<'b>, Option<Url>)>,
) -> Result<()> {
    if parts.len() >= MAX_PARTS {
        bail!("MHTML 存档包含过多资源");
    }
    let location = part
        .headers
        .get_first_value("Content-Location")
        .and_then(|value| resolve_url(&value, inherited_location))
        .or_else(|| inherited_location.cloned());
    parts.push((part, location.clone()));
    for child in &part.subparts {
        collect_parts(child, location.as_ref(), parts)?;
    }
    Ok(())
}

fn html_part<'a, 'b>(part: &'a ParsedMail<'b>) -> Result<Option<&'a ParsedMail<'b>>> {
    if matches!(
        part.ctype.mimetype.as_str(),
        "text/html" | "application/xhtml+xml"
    ) {
        return Ok(Some(part));
    }
    if part.ctype.mimetype == "multipart/related"
        && let Some(start) = part.ctype.params.get("start")
    {
        let root = part
            .subparts
            .iter()
            .find(|child| {
                child
                    .headers
                    .get_first_value("Content-ID")
                    .is_some_and(|id| content_id(&id) == content_id(start))
            })
            .context("MHTML 存档指定的正文资源不存在")?;
        return html_part(root);
    }
    for child in &part.subparts {
        if let Some(root) = html_part(child)? {
            return Ok(Some(root));
        }
    }
    Ok(None)
}

fn decode_html(part: &ParsedMail<'_>) -> Result<String> {
    let bytes = part.get_body_raw().context("MHTML 正文解码失败")?;
    let charset = part.ctype.params.get("charset").cloned().or_else(|| {
        let header = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]);
        let document = Html::parse_document(&header);
        document.tree.nodes().find_map(|node| {
            let element = node.value().as_element()?;
            if element.name() != "meta" {
                return None;
            }
            element.attr("charset").map(str::to_string).or_else(|| {
                element
                    .attr("http-equiv")
                    .filter(|value| value.eq_ignore_ascii_case("content-type"))?;
                let content = element.attr("content")?;
                mailparse::parse_content_type(content)
                    .params
                    .get("charset")
                    .cloned()
            })
        })
    });
    let encoding = match charset {
        Some(charset) => {
            Encoding::for_label(charset.as_bytes()).context("MHTML 正文使用不支持的字符编码")?
        }
        None => Encoding::for_bom(&bytes)
            .map(|(encoding, _)| encoding)
            .unwrap_or(UTF_8),
    };
    let (html, _, had_errors) = encoding.decode(&bytes);
    if had_errors {
        bail!("MHTML 正文字符编码无效");
    }
    Ok(html.into_owned())
}

fn content_id(value: &str) -> &str {
    value.trim().trim_start_matches('<').trim_end_matches('>')
}

fn resolve_url(value: &str, base: Option<&Url>) -> Option<Url> {
    Url::parse(value.trim())
        .ok()
        .or_else(|| base.and_then(|base| base.join(value.trim()).ok()))
}

fn reference_key(value: &str, base: Option<&Url>) -> String {
    let value = value.trim();
    if value
        .get(..4)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("cid:"))
    {
        return format!(
            "cid:{}",
            percent_decode_str(&value[4..]).decode_utf8_lossy()
        );
    }
    if let Some(mut url) = resolve_url(value, base) {
        url.set_fragment(None);
        url.to_string()
    } else {
        value.trim_start_matches("./").to_string()
    }
}

#[cfg(test)]
mod tests;
