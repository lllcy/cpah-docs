use crate::models::MinerUPartMode;
use crate::state::ProfileRuntimeControl;
use anyhow::{Context, Result, bail};
use lopdf::Document;
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};

pub const MINERU_FILE_LIMIT_BYTES: u64 = 200_000_000;
pub const MAX_SOURCE_BYTES: u64 = 1024 * 1024 * 1024;
pub const TARGET_PART_BYTES: u64 = 180_000_000;
pub const MAX_SINGLE_PAGE_PART_BYTES: u64 = 190_000_000;
pub const MAX_PART_PAGES: u32 = 200;
pub const LOW_MEMORY_THRESHOLD_BYTES: u64 = 64 * 1024 * 1024;
const LOCAL_PART_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfPartPlan {
    pub index: u32,
    pub count: u32,
    pub page_start: u32,
    pub page_end: u32,
    pub mode: MinerUPartMode,
    pub input_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PdfPlan {
    Direct {
        page_count: u32,
    },
    Multipart {
        page_count: u32,
        parts: Vec<PdfPartPlan>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PdfStrategy {
    Direct,
    PageRanges,
    SplitPdf,
}

#[cfg(test)]
pub fn plan_pdf(source: &Path, work_dir: &Path) -> Result<PdfPlan> {
    plan_pdf_with_control(source, work_dir, None)
}

pub fn plan_pdf_with_control(
    source: &Path,
    work_dir: &Path,
    control: Option<&ProfileRuntimeControl>,
) -> Result<PdfPlan> {
    let source_size = fs::metadata(source)
        .with_context(|| format!("无法读取 PDF 文件信息：{}", source.display()))?
        .len();
    if source_size > MAX_SOURCE_BYTES {
        bail!("PDF 超过 1 GiB 本地预处理安全上限：{}", source.display());
    }

    crate::qpdf::check_cancelled(control)?;
    let page_count = if source_size > LOW_MEMORY_THRESHOLD_BYTES {
        crate::qpdf::page_count(source, control)?
    } else {
        u32::try_from(load_unencrypted_pdf(source)?.get_pages().len())
            .context("PDF 页数超过支持范围")?
    };
    if page_count == 0 {
        bail!("PDF 不包含可解析页面：{}", source.display());
    }
    match pdf_strategy(source_size, page_count) {
        PdfStrategy::Direct => return Ok(PdfPlan::Direct { page_count }),
        PdfStrategy::PageRanges => {
            return Ok(PdfPlan::Multipart {
                page_count,
                parts: page_range_parts(page_count, MinerUPartMode::PageRanges, None),
            });
        }
        PdfStrategy::SplitPdf => {}
    }

    split_physical(
        source,
        work_dir,
        page_count,
        TARGET_PART_BYTES,
        MAX_PART_PAGES,
        control,
    )
}

pub(crate) fn split_for_local(
    source: &Path,
    work_dir: &Path,
    control: Option<&ProfileRuntimeControl>,
) -> Result<PdfPlan> {
    crate::local_conversion::validate_source_size(source, fs::metadata(source)?.len())?;
    let page_count = crate::qpdf::page_count(source, control)?;
    split_physical(source, work_dir, page_count, LOCAL_PART_BYTES, 50, control)
}

fn split_physical(
    source: &Path,
    work_dir: &Path,
    page_count: u32,
    target_bytes: u64,
    max_pages: u32,
    control: Option<&ProfileRuntimeControl>,
) -> Result<PdfPlan> {
    fs::create_dir_all(work_dir)?;
    let input = tempfile::Builder::new()
        .prefix("input-")
        .tempdir_in(work_dir)?;
    let input_dir = input.path();

    let mut pending = VecDeque::new();
    for start in (1..=page_count).step_by(max_pages as usize) {
        pending.push_back((start, start.saturating_add(max_pages - 1).min(page_count)));
    }
    let mut accepted = Vec::new();
    while let Some((page_start, page_end)) = pending.pop_front() {
        let candidate = input_dir.join(format!("candidate-{page_start:06}-{page_end:06}.pdf"));
        crate::qpdf::check_cancelled(control)?;
        let size =
            crate::qpdf::write_page_range(source, page_start, page_end, &candidate, control)?;
        let is_single_page = page_start == page_end;
        if size <= target_bytes || (is_single_page && size < MAX_SINGLE_PAGE_PART_BYTES) {
            accepted.push((page_start, page_end, candidate));
            continue;
        }
        fs::remove_file(&candidate).ok();
        if is_single_page {
            bail!("PDF 第 {page_start} 页单页分片达到或超过 190 MB，无法安全处理");
        }
        let middle = page_start + (page_end - page_start) / 2;
        pending.push_front((middle + 1, page_end));
        pending.push_front((page_start, middle));
    }
    accepted.sort_by_key(|(page_start, _, _)| *page_start);
    let count = u32::try_from(accepted.len()).context("PDF 分片数量超过支持范围")?;
    let mut parts = Vec::with_capacity(accepted.len());
    for (offset, (page_start, page_end, candidate)) in accepted.into_iter().enumerate() {
        let index = u32::try_from(offset + 1).context("PDF 分片序号超过支持范围")?;
        let input_path = input_dir.join(format!("part-{index:04}.pdf"));
        fs::rename(&candidate, &input_path).with_context(|| {
            format!(
                "无法安装 PDF 分片：{} -> {}",
                candidate.display(),
                input_path.display()
            )
        })?;
        validate_part(&input_path, page_end - page_start + 1, control)?;
        parts.push(PdfPartPlan {
            index,
            count,
            page_start,
            page_end,
            mode: MinerUPartMode::SplitPdf,
            input_path: Some(input_path),
        });
    }
    crate::qpdf::check_cancelled(control)?;
    let _ = input.keep();
    Ok(PdfPlan::Multipart { page_count, parts })
}

fn pdf_strategy(source_size: u64, page_count: u32) -> PdfStrategy {
    if source_size <= MINERU_FILE_LIMIT_BYTES && page_count <= MAX_PART_PAGES {
        PdfStrategy::Direct
    } else if source_size <= MINERU_FILE_LIMIT_BYTES {
        PdfStrategy::PageRanges
    } else {
        PdfStrategy::SplitPdf
    }
}

#[cfg(test)]
pub fn recreate_physical_part(
    source: &Path,
    page_start: u32,
    page_end: u32,
    destination: &Path,
) -> Result<u64> {
    recreate_physical_part_with_control(source, page_start, page_end, destination, None)
}

pub fn recreate_physical_part_with_control(
    source: &Path,
    page_start: u32,
    page_end: u32,
    destination: &Path,
    control: Option<&ProfileRuntimeControl>,
) -> Result<u64> {
    crate::local_conversion::validate_source_size(source, fs::metadata(source)?.len())?;
    let total_pages = crate::qpdf::page_count(source, control)?;
    if page_start == 0 || page_start > page_end || page_end > total_pages {
        bail!("PDF 分片页码范围无效：{page_start}-{page_end}");
    }
    let parent = destination.parent().context("PDF 分片缺少父目录")?;
    fs::create_dir_all(parent)?;
    let staging = tempfile::Builder::new()
        .prefix("recreate-")
        .tempdir_in(parent)?;
    let candidate = staging.path().join("part.pdf");
    let size = crate::qpdf::write_page_range(source, page_start, page_end, &candidate, control)?;
    let page_count = page_end
        .checked_sub(page_start)
        .and_then(|value| value.checked_add(1))
        .context("PDF 分片页码范围无效")?;
    validate_part(&candidate, page_count, control)?;
    if size > TARGET_PART_BYTES && !(page_count == 1 && size < MAX_SINGLE_PAGE_PART_BYTES) {
        bail!("重新生成的 PDF 分片超过安全大小：{}", destination.display());
    }
    crate::qpdf::check_cancelled(control)?;
    fs::rename(candidate, destination).context("无法安装重新生成的 PDF 分片")?;
    Ok(size)
}

fn page_range_parts(
    page_count: u32,
    mode: MinerUPartMode,
    input_path: Option<PathBuf>,
) -> Vec<PdfPartPlan> {
    let ranges = (1..=page_count)
        .step_by(MAX_PART_PAGES as usize)
        .map(|page_start| {
            (
                page_start,
                (page_start + MAX_PART_PAGES - 1).min(page_count),
            )
        })
        .collect::<Vec<_>>();
    let count = ranges.len() as u32;
    ranges
        .into_iter()
        .enumerate()
        .map(|(offset, (page_start, page_end))| PdfPartPlan {
            index: offset as u32 + 1,
            count,
            page_start,
            page_end,
            mode: mode.clone(),
            input_path: input_path.clone(),
        })
        .collect()
}

fn load_unencrypted_pdf(source: &Path) -> Result<Document> {
    let document = Document::load(source)
        .with_context(|| format!("PDF 已损坏或格式不受支持：{}", source.display()))?;
    if document.is_encrypted() {
        bail!("PDF 已加密且需要密码，不支持自动拆分：{}", source.display());
    }
    Ok(document)
}

fn validate_part(
    path: &Path,
    expected_pages: u32,
    control: Option<&ProfileRuntimeControl>,
) -> Result<()> {
    let actual_pages =
        crate::qpdf::page_count(path, control).context("生成的 PDF 分片无法重新读取")?;
    if actual_pages != expected_pages {
        bail!("PDF 分片页数校验失败：期望 {expected_pages} 页，实际 {actual_pages} 页");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{Object, Stream, dictionary};

    fn write_test_pdf(path: &Path, page_count: u32) {
        let mut document = Document::with_version("1.5");
        let pages_id = document.new_object_id();
        let resources_id = document.add_object(dictionary! {});
        let content_id = document.add_object(Stream::new(dictionary! {}, Vec::new()));
        let mut kids = Vec::new();
        for _ in 0..page_count {
            let page_id = document.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "Contents" => content_id,
            });
            kids.push(page_id.into());
        }
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => kids,
                "Count" => i64::from(page_count),
                "Resources" => resources_id,
                "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            }),
        );
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        document.trailer.set("Root", catalog_id);
        document.save(path).unwrap();
    }

    #[test]
    fn page_range_plan_uses_one_based_200_page_chunks() {
        let parts = page_range_parts(401, MinerUPartMode::PageRanges, None);
        assert_eq!(parts.len(), 3);
        assert_eq!((parts[0].page_start, parts[0].page_end), (1, 200));
        assert_eq!((parts[1].page_start, parts[1].page_end), (201, 400));
        assert_eq!((parts[2].page_start, parts[2].page_end), (401, 401));
        assert!(parts.iter().all(|part| part.count == 3));
    }

    #[test]
    fn exact_size_and_page_boundaries_choose_expected_strategy() {
        assert_eq!(
            pdf_strategy(MINERU_FILE_LIMIT_BYTES, MAX_PART_PAGES),
            PdfStrategy::Direct
        );
        assert_eq!(
            pdf_strategy(MINERU_FILE_LIMIT_BYTES, MAX_PART_PAGES + 1),
            PdfStrategy::PageRanges
        );
        assert_eq!(
            pdf_strategy(MINERU_FILE_LIMIT_BYTES + 1, 1),
            PdfStrategy::SplitPdf
        );
    }

    #[test]
    fn plan_keeps_200_pages_direct_and_uses_page_ranges_for_201() {
        let temporary = tempfile::tempdir().unwrap();
        let direct = temporary.path().join("200.pdf");
        let ranged = temporary.path().join("201.pdf");
        write_test_pdf(&direct, 200);
        write_test_pdf(&ranged, 201);

        assert!(matches!(
            plan_pdf(&direct, &temporary.path().join("direct-work")).unwrap(),
            PdfPlan::Direct { page_count: 200 }
        ));
        let PdfPlan::Multipart { page_count, parts } =
            plan_pdf(&ranged, &temporary.path().join("ranged-work")).unwrap()
        else {
            panic!("201 pages should use page_ranges")
        };
        assert_eq!(page_count, 201);
        assert_eq!(parts.len(), 2);
        assert_eq!((parts[0].page_start, parts[0].page_end), (1, 200));
        assert_eq!((parts[1].page_start, parts[1].page_end), (201, 201));
        assert!(
            parts
                .iter()
                .all(|part| part.mode == MinerUPartMode::PageRanges && part.input_path.is_none())
        );
    }

    #[test]
    fn source_larger_than_one_gib_is_rejected_before_parsing() {
        let temporary = tempfile::tempdir().unwrap();
        let source = temporary.path().join("too-large.pdf");
        let file = fs::File::create(&source).unwrap();
        file.set_len(MAX_SOURCE_BYTES + 1).unwrap();
        let error = plan_pdf(&source, &temporary.path().join("work")).unwrap_err();
        assert!(format!("{error:#}").contains("1 GiB"));
    }

    #[test]
    fn pdfs_between_old_limit_and_one_gib_select_physical_splitting() {
        for size in [512 * 1024 * 1024 + 1, MAX_SOURCE_BYTES] {
            assert_eq!(pdf_strategy(size, 250), PdfStrategy::SplitPdf);
        }
    }

    #[test]
    #[ignore = "requires CPAHDOCS_LARGE_PDF_E2E to point to the generated large PDF"]
    fn large_pdf_above_old_limit_converts_and_splits_locally() {
        let source = PathBuf::from(std::env::var("CPAHDOCS_LARGE_PDF_E2E").unwrap());
        let size = fs::metadata(&source).unwrap().len();
        assert!(size > 512 * 1024 * 1024 && size <= MAX_SOURCE_BYTES);
        let result = crate::local_conversion::convert(&source).unwrap();
        let crate::local_conversion::LocalConversion::Converted(artifact) = result else {
            panic!("expected local extraction of the synthetic text PDF");
        };
        assert!(artifact.markdown.contains("Large PDF regression fixture"));
        let temporary = tempfile::tempdir().unwrap();
        let PdfPlan::Multipart { page_count, parts } = plan_pdf(&source, temporary.path()).unwrap()
        else {
            panic!("expected physical splitting above the upload limit");
        };
        assert_eq!(page_count, 1);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].mode, MinerUPartMode::SplitPdf);
        let part = parts[0].input_path.as_ref().unwrap();
        assert!(fs::metadata(part).unwrap().len() <= TARGET_PART_BYTES);
        let crate::local_conversion::LocalConversion::Converted(split_artifact) =
            crate::local_conversion::convert(part).unwrap()
        else {
            panic!("expected local extraction after lossless splitting");
        };
        assert!(
            split_artifact
                .markdown
                .contains("Large PDF regression fixture")
        );
    }

    #[test]
    #[ignore = "requires CPAHDOCS_REFERENCED_PDF_E2E generated 64-page fixture"]
    fn qpdf_large_pdf_plans_and_recreates_mineru_inputs() {
        let source = PathBuf::from(std::env::var("CPAHDOCS_REFERENCED_PDF_E2E").unwrap());
        assert!(fs::metadata(&source).unwrap().len() > 512 * 1024 * 1024);
        let temporary = tempfile::tempdir().unwrap();
        let PdfPlan::Multipart { page_count, parts } = plan_pdf(&source, temporary.path()).unwrap()
        else {
            panic!("large source must be physically split");
        };
        assert_eq!(page_count, 64);
        assert!(parts.len() > 1);
        let mut expected_start = 1;
        for part in &parts {
            assert_eq!(part.page_start, expected_start);
            expected_start = part.page_end + 1;
            assert_eq!(part.mode, MinerUPartMode::SplitPdf);
            let input = part.input_path.as_ref().unwrap();
            assert!(fs::metadata(input).unwrap().len() <= TARGET_PART_BYTES);
            assert_eq!(
                crate::qpdf::page_count(input, None).unwrap(),
                part.page_end - part.page_start + 1
            );
        }
        assert_eq!(expected_start, 65);
        let first = &parts[0];
        let input = first.input_path.as_ref().unwrap();
        fs::remove_file(input).unwrap();
        recreate_physical_part(&source, first.page_start, first.page_end, input).unwrap();
        assert!(fs::metadata(input).unwrap().len() <= TARGET_PART_BYTES);
        let crate::local_conversion::LocalConversion::Converted(artifact) =
            crate::local_conversion::convert(input).unwrap()
        else {
            panic!("recreated text pages must remain readable");
        };
        assert!(artifact.markdown.contains("SourcePage-0001"));
        assert!(
            artifact
                .markdown
                .contains(&format!("SourcePage-{:04}", first.page_end))
        );
        eprintln!(
            "QPDF MinerU plan validated: {} parts, {page_count} pages; first part regenerated and converted",
            parts.len()
        );
    }

    #[test]
    fn physical_page_range_preserves_requested_contiguous_pages() {
        let temporary = tempfile::tempdir().unwrap();
        let source = temporary.path().join("source.pdf");
        let destination = temporary.path().join("part.pdf");
        write_test_pdf(&source, 10);

        recreate_physical_part(&source, 3, 7, &destination).unwrap();
        let part = Document::load(&destination).unwrap();
        assert_eq!(part.get_pages().len(), 5);
        assert!(fs::metadata(destination).unwrap().len() <= TARGET_PART_BYTES);
    }

    #[test]
    #[ignore = "requires locally generated MinerU PDF E2E fixtures"]
    fn generated_e2e_fixtures_follow_expected_plans() {
        let boundary = PathBuf::from(std::env::var("CPAHDOCS_MINERU_BOUNDARY_E2E").unwrap());
        let page_ranges = PathBuf::from(std::env::var("CPAHDOCS_MINERU_PAGE_RANGES_E2E").unwrap());
        let physical = PathBuf::from(std::env::var("CPAHDOCS_MINERU_PHYSICAL_E2E").unwrap());
        let temporary = tempfile::tempdir().unwrap();

        assert!(matches!(
            plan_pdf(&boundary, &temporary.path().join("boundary")).unwrap(),
            PdfPlan::Direct { page_count: 200 }
        ));

        let PdfPlan::Multipart { page_count, parts } =
            plan_pdf(&page_ranges, &temporary.path().join("page-ranges")).unwrap()
        else {
            panic!("201-page fixture should use page_ranges")
        };
        assert_eq!(page_count, 201);
        assert_eq!(parts.len(), 2);
        assert_eq!((parts[0].page_start, parts[0].page_end), (1, 200));
        assert_eq!((parts[1].page_start, parts[1].page_end), (201, 201));
        assert!(
            parts.iter().all(|part| {
                part.mode == MinerUPartMode::PageRanges && part.input_path.is_none()
            })
        );

        let PdfPlan::Multipart { page_count, parts } =
            plan_pdf(&physical, &temporary.path().join("physical")).unwrap()
        else {
            panic!(">200 MB fixture should use physical splitting")
        };
        assert_eq!(page_count, 4);
        assert_eq!(parts.len(), 2);
        assert_eq!((parts[0].page_start, parts[0].page_end), (1, 2));
        assert_eq!((parts[1].page_start, parts[1].page_end), (3, 4));
        assert!(parts.iter().all(|part| {
            part.mode == MinerUPartMode::SplitPdf
                && part.input_path.as_ref().is_some_and(|path| {
                    fs::metadata(path).is_ok_and(|metadata| metadata.len() <= TARGET_PART_BYTES)
                })
        }));
    }
}
