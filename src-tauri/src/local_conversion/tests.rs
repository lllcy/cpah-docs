use super::*;
use anydoc::model::{Asset, AssetId, Cell, LinkTarget, Note, NoteKind, Table, TableKind};
use lopdf::{Document as PdfDocument, Object, Stream, dictionary};
use std::fs;
use std::io::{Cursor, Write};
use zip::{ZipWriter, write::SimpleFileOptions};

const IMAGE: &[u8] = b"synthetic image payload preserved verbatim";

pub(crate) fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        zip.start_file(*name, SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes).unwrap();
    }
    fs::write(path, zip.finish().unwrap().into_inner()).unwrap();
}

pub(crate) fn write_docx(path: &Path) {
    write_zip(path, &[
        ("word/document.xml", br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:p><w:r><w:t>Document fixture</w:t></w:r></w:p><w:p><w:r><w:drawing><a:blip r:embed="image1"/></w:drawing></w:r></w:p><w:tbl><w:tr><w:tc><w:p><w:r><w:t>Table cell</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:body></w:document>"#),
        ("word/_rels/document.xml.rels", br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="image1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image1.png"/></Relationships>"#),
        ("word/media/image1.png", IMAGE),
    ]);
}

// Every fixture is generated from public format primitives; no user documents.
// false = text page, true = image-only page.
pub(crate) fn pdf_document(scanned: &[bool]) -> PdfDocument {
    let mut doc = PdfDocument::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica"
    });
    let image_id = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Image", "Width" => 2, "Height" => 2,
            "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8
        },
        vec![128; 12],
    ));
    let mut kids = Vec::new();
    for is_scan in scanned {
        let resources_id = doc.add_object(if *is_scan {
            dictionary! { "XObject" => dictionary! { "Im1" => image_id } }
        } else {
            dictionary! { "Font" => dictionary! { "F1" => font_id } }
        });
        let content = if *is_scan {
            b"q 595 0 0 842 0 0 cm /Im1 Do Q".to_vec()
        } else {
            b"BT /F1 12 Tf 50 750 Td (Local PDF fixture with enough readable text to test native extraction without a cloud token.) Tj ET".to_vec()
        };
        let content_id = doc.add_object(Stream::new(dictionary! {}, content));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => content_id,
            "Resources" => resources_id, "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()]
        });
        kids.push(Object::Reference(page_id));
    }
    doc.objects.insert(
        pages_id,
        dictionary! {
            "Type" => "Pages", "Kids" => kids, "Count" => scanned.len() as i64
        }
        .into(),
    );
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);
    doc
}

pub(crate) fn write_pdf(path: &Path, scanned: &[bool]) {
    pdf_document(scanned).save(path).unwrap();
}

fn artifact(path: &Path) -> ConversionArtifact {
    match convert(path).unwrap() {
        LocalConversion::Converted(artifact) => artifact,
        other => panic!("unexpected OCR request: {other:?}"),
    }
}

#[test]
fn html_htm_and_txt_keep_anytomd_content() {
    let temp = tempfile::tempdir().unwrap();
    for ext in ["html", "HTM"] {
        let path = temp.path().join(format!("sample.{ext}"));
        fs::write(
            &path,
            "<h1>网页标题</h1><p>Hello <a href=\"https://example.com\">link</a></p>",
        )
        .unwrap();
        let result = artifact(&path);
        assert!(result.markdown.contains("网页标题"));
        assert!(result.markdown.contains("[link](https://example.com)"));
    }
    let path = temp.path().join("sample.txt");
    fs::write(&path, "纯文本\nSecond line\n").unwrap();
    assert!(artifact(&path).markdown.contains("纯文本\nSecond line"));
}

#[test]
fn docx_keeps_table_and_embedded_image_bytes() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("sample.docx");
    write_docx(&source);
    let result = artifact(&source);
    assert!(result.markdown.contains("Document fixture"));
    assert!(result.markdown.contains("Table cell"));
    assert!(
        result.markdown.contains("asset-0000.png"),
        "{}",
        result.markdown
    );
    assert_eq!(result.assets.len(), 1);
    assert_eq!(result.assets[0].bytes, IMAGE);
    assert_eq!(result.assets[0].relative_path, Path::new("asset-0000.png"));
}

#[test]
fn xlsx_and_pptx_preserve_embedded_images() {
    let temp = tempfile::tempdir().unwrap();
    let xlsx = temp.path().join("sample.xlsx");
    write_zip(&xlsx, &[
        ("xl/workbook.xml", br#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Data" sheetId="1" r:id="s1"/></sheets></workbook>"#),
        ("xl/_rels/workbook.xml.rels", br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="s1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#),
        ("xl/worksheets/sheet1.xml", br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Sheet fixture</t></is></c></row></sheetData><drawing r:id="d1"/></worksheet>"#),
        ("xl/worksheets/_rels/sheet1.xml.rels", br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="d1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing" Target="../drawings/drawing1.xml"/></Relationships>"#),
        ("xl/drawings/drawing1.xml", br#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><xdr:oneCellAnchor><xdr:from><xdr:col>0</xdr:col><xdr:row>1</xdr:row></xdr:from><xdr:pic><xdr:blipFill><a:blip r:embed="img"/></xdr:blipFill></xdr:pic></xdr:oneCellAnchor></xdr:wsDr>"#),
        ("xl/drawings/_rels/drawing1.xml.rels", br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="img" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image1.png"/></Relationships>"#),
        ("xl/media/image1.png", IMAGE),
    ]);
    let pptx = temp.path().join("sample.pptx");
    write_zip(&pptx, &[
        ("ppt/presentation.xml", br#"<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sldIdLst><p:sldId id="256" r:id="s1"/></p:sldIdLst></p:presentation>"#),
        ("ppt/_rels/presentation.xml.rels", br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="s1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="/ppt/slides/slide1.xml"/></Relationships>"#),
        ("ppt/slides/slide1.xml", br#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>Slide fixture</a:t></a:r></a:p></p:txBody></p:sp><p:pic><p:blipFill><a:blip r:embed="img"/></p:blipFill></p:pic></p:spTree></p:cSld></p:sld>"#),
        ("ppt/slides/_rels/slide1.xml.rels", br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="img" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image1.png"/></Relationships>"#),
        ("ppt/media/image1.png", IMAGE),
    ]);
    for (path, text) in [(xlsx, "Sheet fixture"), (pptx, "Slide fixture")] {
        let result = artifact(&path);
        assert!(result.markdown.contains(text));
        assert!(
            result.markdown.contains("asset-0000.png"),
            "{}",
            result.markdown
        );
        assert_eq!(result.assets[0].bytes, IMAGE);
    }
}

#[test]
fn csv_rtf_opendocument_and_epub_extract_text() {
    let temp = tempfile::tempdir().unwrap();
    for (ext, input, expected) in [
        ("csv", "Name,Value\nAlpha,42\n", "Alpha"),
        ("rtf", "{\\rtf1\\ansi RTF fixture\\par}", "RTF fixture"),
    ] {
        let path = temp.path().join(format!("sample.{ext}"));
        fs::write(&path, input).unwrap();
        assert!(artifact(&path).markdown.contains(expected));
    }
    for ext in ["odt", "ods", "odp"] {
        let path = temp.path().join(format!("sample.{ext}"));
        write_zip(&path, &[("content.xml", br#"<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0"><office:body><office:text><text:p>ODF fixture</text:p></office:text><office:spreadsheet><table:table table:name="Data"><table:table-row><table:table-cell office:value-type="string"><text:p>ODF fixture</text:p></table:table-cell></table:table-row></table:table></office:spreadsheet><office:presentation><draw:page draw:name="Slide"><draw:frame><draw:text-box><text:p>ODF fixture</text:p></draw:text-box></draw:frame></draw:page></office:presentation></office:body></office:document-content>"#)]);
        assert!(artifact(&path).markdown.contains("ODF fixture"));
    }
    let epub = temp.path().join("sample.epub");
    write_zip(&epub, &[
        ("META-INF/container.xml", br#"<container><rootfiles><rootfile full-path="book.opf"/></rootfiles></container>"#),
        ("book.opf", br#"<package><manifest><item id="c1" href="chapter.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#),
        ("chapter.xhtml", br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>EPUB fixture</p></body></html>"#),
    ]);
    assert!(artifact(&epub).markdown.contains("EPUB fixture"));
}

#[test]
fn nested_images_and_notes_are_resolved_and_missing_images_warn() {
    let image = || Inline::Image {
        alt: "Picture".into(),
        source: ImageSource::Asset(AssetId(0)),
    };
    let mut doc = Document::default();
    doc.assets.push(Asset {
        id: AssetId(0),
        media_type: "image/png".into(),
        origin_part: "../image.png".into(),
        bytes: IMAGE.to_vec(),
    });
    doc.blocks = vec![
        Block::Table(Table::from_rows(
            vec![vec![Cell::from_inlines(vec![Inline::Link {
                content: vec![image()],
                target: LinkTarget::External("https://example.com".into()),
            }])]],
            0,
            TableKind::Data,
        )),
        Block::Paragraph(vec![
            Inline::NoteRef("footnote".into()),
            Inline::Math("x^2".into()),
            Inline::Image {
                alt: "Missing".into(),
                source: ImageSource::Asset(AssetId(9)),
            },
        ]),
    ];
    doc.notes.push(Note {
        id: "footnote".into(),
        kind: NoteKind::Footnote,
        blocks: vec![Block::BlockQuote(vec![Block::Paragraph(vec![image()])])],
    });
    let result = render_document(doc).unwrap();
    assert_eq!(
        result.markdown.matches("asset-0000.png").count(),
        2,
        "{}",
        result.markdown
    );
    assert!(result.markdown.contains("x^2"));
    assert!(result.markdown.contains("Missing"));
    assert_eq!(result.warnings.len(), 1);
}

#[test]
fn pdf_only_requests_ocr_for_scanned_or_mixed_content() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("sample.pdf");
    write_pdf(&path, &[false]);
    let result = artifact(&path);
    assert!(result.markdown.contains("Local PDF fixture"));
    assert!(result.assets.is_empty());
    for (pages, expected) in [(vec![true], vec![1]), (vec![false, true], vec![2])] {
        write_pdf(&path, &pages);
        match convert(&path).unwrap() {
            LocalConversion::NeedsOcr {
                pages: flagged,
                page_count,
            } => {
                assert_eq!(flagged, expected);
                assert_eq!(page_count, pages.len() as u32);
            }
            other => panic!("expected OCR: {other:?}"),
        }
    }
    fs::write(&path, b"%PDF-1.5\nmalformed").unwrap();
    assert!(convert(&path).is_err());
}

#[test]
fn encrypted_and_oversized_pdfs_fail_without_requesting_ocr() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("protected.pdf");
    let mut doc = pdf_document(&[false]);
    doc.trailer.set(
        "ID",
        vec![
            Object::string_literal("synthetic-document-id"),
            Object::string_literal("synthetic-document-id"),
        ],
    );
    let encryption = lopdf::EncryptionState::try_from(lopdf::EncryptionVersion::V2 {
        document: &doc,
        owner_password: "fixture-owner",
        user_password: "fixture-user",
        key_length: 128,
        permissions: lopdf::Permissions::all(),
    })
    .unwrap();
    doc.encrypt(&encryption).unwrap();
    doc.save(&path).unwrap();
    let error = convert(&path).unwrap_err();
    assert!(
        matches!(
            error.downcast_ref::<anydoc::ConvertError>(),
            Some(anydoc::ConvertError::Encrypted)
        ),
        "{error:#}"
    );

    let file = fs::File::create(&path).unwrap();
    file.set_len(MAX_INPUT_BYTES + 1).unwrap();
    drop(file);
    assert!(convert(&path).unwrap_err().to_string().contains("512 MiB"));
}
