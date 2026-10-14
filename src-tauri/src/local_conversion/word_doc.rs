use super::{convert_text, render_document};
use crate::converter::ConversionArtifact;
use anyhow::{Context, Result, bail};
use encoding_rs::{Encoding, UTF_8};
use scraper::Html;

const HEADER_BYTES: usize = 8192;

pub(super) fn convert(bytes: &[u8], fallback: anydoc::Format) -> Result<ConversionArtifact> {
    // Word exports can contain HTML, RTF or a Word container regardless of
    // whether the filename ends in .doc, .docx or .docm.
    // Select only recognizable Word formats; damaged binary files must still
    // report a parsing error instead of silently yielding arbitrary text.
    if let Some(format @ (anydoc::Format::Doc | anydoc::Format::Rtf | anydoc::Format::Docx)) =
        anydoc::Format::from_bytes(bytes)
    {
        return render_word(bytes, format);
    }
    let header = &bytes[..bytes.len().min(HEADER_BYTES)];
    let prefix = if let Some((encoding, _)) = Encoding::for_bom(bytes) {
        encoding.decode(header).0
    } else {
        String::from_utf8_lossy(header)
    };
    if html_signature(&prefix) {
        let html = decode_html(bytes, &prefix)?;
        return convert_text(html.as_bytes(), "html");
    }
    render_word(bytes, fallback)
}

fn render_word(bytes: &[u8], format: anydoc::Format) -> Result<ConversionArtifact> {
    let document = anydoc::to_document(bytes, format).context("anydoc Word 文档解析失败")?;
    render_document(document)
}

fn html_signature(prefix: &str) -> bool {
    let mut prefix = prefix.trim_start_matches(|ch: char| ch.is_whitespace() || ch == '\u{feff}');
    loop {
        if let Some(comment) = prefix.strip_prefix("<!--") {
            let Some(end) = comment.find("-->") else {
                return false;
            };
            prefix = comment[end + 3..].trim_start();
        } else if prefix.starts_with("<?") {
            let Some(end) = prefix.find("?>") else {
                return false;
            };
            prefix = prefix[end + 2..].trim_start();
        } else {
            break;
        }
    }
    let prefix = prefix.to_ascii_lowercase();
    ["<html", "<!doctype html"].iter().any(|tag| {
        prefix
            .strip_prefix(tag)
            .is_some_and(|rest| rest.starts_with(|ch: char| ch.is_ascii_whitespace() || ch == '>'))
    })
}

fn decode_html(bytes: &[u8], prefix: &str) -> Result<String> {
    let encoding = if let Some((encoding, _)) = Encoding::for_bom(bytes) {
        encoding
    } else {
        let header = Html::parse_document(prefix);
        let charset = header.tree.nodes().find_map(|node| {
            let element = node.value().as_element()?;
            if element.name() != "meta" {
                return None;
            }
            element.attr("charset").map(str::to_string).or_else(|| {
                element
                    .attr("http-equiv")
                    .filter(|value| value.eq_ignore_ascii_case("content-type"))?;
                mailparse::parse_content_type(element.attr("content")?)
                    .params
                    .get("charset")
                    .cloned()
            })
        });
        match charset {
            Some(charset) => Encoding::for_label(charset.as_bytes())
                .context("Word 网页正文使用不支持的字符编码")?,
            None => UTF_8,
        }
    };
    let (html, _, errors) = encoding.decode(bytes);
    if errors {
        bail!("Word 网页正文字符编码无效");
    }
    Ok(html.into_owned())
}
