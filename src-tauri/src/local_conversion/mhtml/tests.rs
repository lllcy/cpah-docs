use super::*;
use crate::converter::LocalConversion;
use std::fs;

fn archive(parameters: &str, parts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut bytes = format!(
        "MIME-Version: 1.0\r\nContent-Type: multipart/related;\r\n boundary=\"saved-page\"{parameters}\r\n\r\n"
    ).into_bytes();
    for (headers, body) in parts {
        bytes.extend_from_slice(format!("--saved-page\r\n{headers}\r\n\r\n").as_bytes());
        bytes.extend_from_slice(body);
        bytes.extend_from_slice(b"\r\n");
    }
    bytes.extend_from_slice(b"--saved-page--\r\n");
    bytes
}

fn quoted_printable(bytes: &[u8]) -> String {
    bytes
        .chunks(24)
        .map(|chunk| {
            chunk
                .iter()
                .map(|byte| format!("={byte:02X}"))
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("=\r\n")
}

#[test]
fn browser_archive_keeps_chinese_tables_links_and_embedded_image_bytes() {
    let html = quoted_printable(
        concat!(
            "<h1>网页标题</h1><p><a href='../about'>链接</a><a href='#section'>锚点</a></p>",
            "<table><tr><th>项目</th></tr><tr><td>金额</td></tr></table>",
            "<img src='cid:photo@cpah' alt='照片'><img src='CID:photo%40cpah' alt='重复照片'>",
            "<img src='../media/chart.png' alt='图表'>"
        )
        .as_bytes(),
    );
    let image = [0, 255, 10, 42];
    let encoded_image = quoted_printable(&image);
    let bytes = archive(
        "; start=\"<main>\"",
        &[
            (
                "Content-Type: text/html; charset=utf-8",
                b"<h1>Unselected frame</h1>",
            ),
            (
                "Content-Type: text/html; charset=utf-8\r\nContent-ID: <main>\r\nContent-Location: https://example.com/articles/page.html\r\nContent-Transfer-Encoding: quoted-printable",
                html.as_bytes(),
            ),
            (
                "Content-Type: image/png\r\nContent-ID: <photo@cpah>\r\nContent-Disposition: inline; filename=\"../../escape.png\"\r\nContent-Transfer-Encoding: base64",
                b"cGljdHVyZQ==",
            ),
            (
                "Content-Type: image/png\r\nContent-Location: https://example.com/media/chart.png\r\nContent-Transfer-Encoding: quoted-printable",
                encoded_image.as_bytes(),
            ),
        ],
    );
    let artifact = convert(&bytes).unwrap();
    assert!(
        artifact.markdown.contains("# 网页标题"),
        "{}",
        artifact.markdown
    );
    assert!(!artifact.markdown.contains("Unselected frame"));
    assert!(
        artifact
            .markdown
            .contains("[链接](https://example.com/about)")
    );
    assert!(artifact.markdown.contains("[锚点](#section)"));
    assert!(artifact.markdown.contains("| 项目 |"));
    assert!(artifact.markdown.contains("金额"));
    assert_eq!(artifact.assets.len(), 2);
    assert_eq!(artifact.assets[0].bytes, b"picture");
    assert_eq!(artifact.assets[1].bytes, image);
    assert_eq!(
        artifact.assets[0].relative_path,
        PathBuf::from("mhtml-0000.png")
    );
    assert_eq!(artifact.markdown.matches("mhtml-0000.png").count(), 2);
    assert!(artifact.markdown.contains("![图表](mhtml-0001.png)"));
    assert!(artifact.warnings.is_empty());
}

#[test]
fn mhtml_and_uppercase_mht_convert_base64_html_through_the_file_entry_point() {
    let bytes = archive(
        "",
        &[(
            "Content-Type: text/html; charset=utf-8\r\nContent-Transfer-Encoding: base64",
            b"PGgxPkhlbGxvPC9oMT4=",
        )],
    );
    let temporary = tempfile::tempdir().unwrap();
    for extension in ["mhtml", "MHT"] {
        let source = temporary.path().join(format!("page.{extension}"));
        fs::write(&source, &bytes).unwrap();
        let LocalConversion::Converted(artifact) =
            crate::local_conversion::convert(&source).unwrap()
        else {
            panic!("a web archive must be converted locally");
        };
        assert!(artifact.markdown.contains("# Hello"));
    }
}

#[test]
fn chinese_gbk_decodes_from_mime_charset_or_html_meta() {
    for (headers, html) in [
        ("Content-Type: text/html; charset=gbk", "<h1>中文存档</h1>"),
        (
            "Content-Type: text/html",
            "<meta charset='gbk'><h1>中文存档</h1>",
        ),
        (
            "Content-Type: text/html",
            "<meta http-equiv='Content-Type' content='text/html; charset=gbk'><h1>中文存档</h1>",
        ),
    ] {
        let (encoded, _, had_errors) = encoding_rs::GBK.encode(html);
        assert!(!had_errors);
        let body = quoted_printable(&encoded);
        let headers = format!("{headers}\r\nContent-Transfer-Encoding: quoted-printable");
        let artifact = convert(&archive("", &[(headers.as_str(), body.as_bytes())])).unwrap();
        assert!(
            artifact.markdown.contains("# 中文存档"),
            "{}",
            artifact.markdown
        );
    }
}

#[test]
fn nested_alternative_root_and_utf8_without_charset_are_supported() {
    let nested = concat!(
        "--alternative\r\nContent-Type: text/plain\r\n\r\nPlain fallback\r\n",
        "--alternative\r\nContent-Type: text/html\r\n\r\n<h1>中文正文</h1>\r\n",
        "--alternative--\r\n"
    );
    let bytes = archive(
        "; start=\"<root>\"",
        &[
            ("Content-Type: text/html", b"<h1>Other frame</h1>"),
            (
                "Content-Type: multipart/alternative; boundary=alternative\r\nContent-ID: <root>",
                nested.as_bytes(),
            ),
        ],
    );
    let artifact = convert(&bytes).unwrap();
    assert!(artifact.markdown.contains("# 中文正文"));
    assert!(!artifact.markdown.contains("Other frame"));
    assert!(!artifact.markdown.contains("Plain fallback"));
}

#[test]
fn base_element_resolves_relative_resources_and_links() {
    let bytes = archive("", &[
        ("Content-Type: text/html\r\nContent-Location: https://example.com/page.html", b"<base href='/saved/'><h1>Page</h1><img src='photo.png'><a href='next.html'>Next</a>"),
        ("Content-Type: image/png\r\nContent-Location: https://example.com/saved/photo.png\r\nContent-Transfer-Encoding: base64", b"cGljdHVyZQ=="),
    ]);
    let artifact = convert(&bytes).unwrap();
    assert!(artifact.markdown.contains("![](mhtml-0000.png)"));
    assert!(
        artifact
            .markdown
            .contains("[Next](https://example.com/saved/next.html)")
    );
    assert_eq!(artifact.assets[0].bytes, b"picture");
}

#[test]
fn resource_limits_count_duplicate_references_once_and_ignore_unused_images() {
    let bytes = archive(
        "",
        &[
            (
                "Content-Type: text/html",
                b"<h1>Page</h1><img src='cid:photo'><img src='cid:photo'>",
            ),
            (
                "Content-Type: image/png\r\nContent-ID: <photo>\r\nContent-Transfer-Encoding: base64",
                b"cGljdHVyZQ==",
            ),
            (
                "Content-Type: image/png\r\nContent-ID: <unused>\r\nContent-Transfer-Encoding: base64",
                b"invalid!",
            ),
        ],
    );
    assert!(convert_with_asset_limit(&bytes, 6).is_err());
    let artifact = convert_with_asset_limit(&bytes, 7).unwrap();
    assert_eq!(artifact.assets.len(), 1);
    assert_eq!(artifact.markdown.matches("mhtml-0000.png").count(), 2);
}

#[test]
fn missing_embedded_images_warn_and_external_images_keep_their_urls() {
    let bytes = archive(
        "",
        &[(
            "Content-Type: text/html",
            b"<h1>Page</h1><img src='cid:missing'><img src='https://example.com/external.png'>",
        )],
    );
    let artifact = convert(&bytes).unwrap();
    assert!(artifact.assets.is_empty());
    assert!(
        artifact
            .markdown
            .contains("https://example.com/external.png")
    );
    assert_eq!(artifact.warnings.len(), 1);
}

#[test]
fn archives_without_html_with_missing_roots_or_invalid_encoding_fail_locally() {
    for bytes in [
        archive("", &[("Content-Type: text/plain", b"Not a web page")]),
        archive(
            "; start=\"<missing>\"",
            &[("Content-Type: text/html", b"<h1>Page</h1>")],
        ),
        archive(
            "",
            &[(
                "Content-Type: text/html; charset=unknown-charset",
                b"<h1>Page</h1>",
            )],
        ),
        archive(
            "",
            &[("Content-Type: text/html; charset=utf-8", b"<h1>\xff</h1>")],
        ),
        archive(
            "",
            &[
                (
                    "Content-Type: text/html",
                    b"<h1>Page</h1><img src='cid:invalid'>",
                ),
                (
                    "Content-Type: image/png\r\nContent-ID: <invalid>\r\nContent-Transfer-Encoding: base64",
                    b"invalid!",
                ),
            ],
        ),
    ] {
        assert!(convert(&bytes).is_err());
    }
}

#[test]
fn archive_part_count_is_bounded() {
    let mut parts = vec![("Content-Type: text/html", b"<h1>Page</h1>".as_slice())];
    parts.extend(std::iter::repeat_n(
        ("Content-Type: text/plain", b"unused".as_slice()),
        MAX_PARTS,
    ));
    let error = convert(&archive("", &parts)).unwrap_err();
    assert!(error.to_string().contains("过多资源"));
}
