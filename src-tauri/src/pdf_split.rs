//! 本地 PDF 拆分工具：用于在调用 MinerU 前将超过页数限制的 PDF 拆分为多个较小的子文件。
//!
//! MinerU 远程 API 对单次解析的 PDF 页数有限制（约 200 页），且其上传接口不接受页码范围参数，
//! 因此只能在本地把 PDF 物理拆分成若干子文件，再依次提交解析，最后由调用方合并结果。

use anyhow::{Context, Result, bail};
use lopdf::Document;
use std::path::{Path, PathBuf};

/// 原始文档中的页码区间（1-based，闭区间）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageRange {
    pub start: u32,
    pub end: u32,
}

/// 统计 PDF 的页数。
pub fn count_pages(path: &Path) -> Result<u32> {
    let doc =
        Document::load(path).with_context(|| format!("无法读取 PDF 页数：{}", path.display()))?;
    Ok(doc.get_pages().len() as u32)
}

/// 计算拆分后的各块页码区间（不含重叠逻辑外的边界处理）。
///
/// 每块最多 `max_pages` 页，相邻块重叠 `overlap` 页以保证跨块内容连贯。
/// 返回 `[(start, end), ...]`，均为 1-based 闭区间。
pub fn plan_chunks(total: u32, max_pages: u32, overlap: u32) -> Vec<(u32, u32)> {
    let max_pages = max_pages.max(1);
    if total == 0 {
        return Vec::new();
    }
    if total <= max_pages {
        return vec![(1, total)];
    }
    // 重叠页数不能超过单块页数，避免步长退化为 0 或产生过多碎片。
    let overlap = overlap.min(max_pages.saturating_sub(1));
    let step = (max_pages - overlap).max(1);
    let mut chunks = Vec::new();
    let mut start = 1u32;
    loop {
        let end = (start + max_pages - 1).min(total);
        chunks.push((start, end));
        if end >= total {
            break;
        }
        start += step;
        if start > total {
            break;
        }
    }
    chunks
}

/// 将 `source` 拆分为多个不超过 `max_pages` 页（含 `overlap` 重叠页）的子 PDF。
///
/// - 若总页数不超过 `max_pages`，则不做拆分，直接返回原始文件与完整页码区间。
/// - 否则在 `out_dir` 下生成 `源文件名_partN.pdf`，每个文件对应一个 [`PageRange`]。
/// - 调用方负责在合并完成后清理 `out_dir`。
pub fn split_pdf(
    source: &Path,
    out_dir: &Path,
    max_pages: u32,
    overlap: u32,
) -> Result<Vec<(PathBuf, PageRange)>> {
    let total = count_pages(source)?;
    if total == 0 {
        bail!("PDF 不包含任何页面：{}", source.display());
    }
    if total <= max_pages {
        return Ok(vec![(
            source.to_path_buf(),
            PageRange {
                start: 1,
                end: total,
            },
        )]);
    }

    let ranges = plan_chunks(total, max_pages, overlap);
    std::fs::create_dir_all(out_dir)
        .with_context(|| format!("无法创建拆分临时目录：{}", out_dir.display()))?;
    let stem = source
        .file_stem()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "document".to_string());

    let mut chunks = Vec::with_capacity(ranges.len());
    for (index, (start, end)) in ranges.iter().enumerate() {
        let out_path = out_dir.join(format!("{stem}_part{}.pdf", index + 1));
        let mut doc = Document::load(source)
            .with_context(|| format!("无法加载 PDF 进行拆分：{}", source.display()))?;
        // 删除区间 [start, end] 之外的所有页，保留需要解析的连续页块。
        let remove: Vec<u32> = (1..=total).filter(|page| *page < *start || *page > *end).collect();
        doc.delete_pages(&remove);
        doc.save(&out_path)
            .with_context(|| format!("无法写入拆分文件：{}", out_path.display()))?;
        chunks.push((
            out_path,
            PageRange {
                start: *start,
                end: *end,
            },
        ));
    }
    Ok(chunks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_single_chunk_when_within_limit() {
        assert_eq!(plan_chunks(10, 200, 5), vec![(1, 10)]);
        assert_eq!(plan_chunks(200, 200, 5), vec![(1, 200)]);
    }

    #[test]
    fn plans_overlapping_chunks_when_exceeding_limit() {
        // 250 页，每块 200，重叠 5 -> 步长 195
        let chunks = plan_chunks(250, 200, 5);
        assert_eq!(chunks, vec![(1, 200), (196, 250)]);
    }

    #[test]
    fn plans_contiguous_coverage_without_gaps() {
        let chunks = plan_chunks(1000, 200, 5);
        assert_eq!(chunks.first(), Some(&(1, 200)));
        assert_eq!(chunks.last(), Some(&(996, 1000)));
        // 相邻块之间重叠 5 页，无断层
        for window in chunks.windows(2) {
            let (_, prev_end) = window[0];
            let (next_start, _) = window[1];
            assert!(next_start <= prev_end + 1, "块之间出现断层：{:?}", window);
        }
    }

    #[test]
    fn clamps_extreme_overlap_to_avoid_infinite_loop() {
        // overlap 大于 max_pages 时被钳制，不应产生异常多的碎片或死循环
        let chunks = plan_chunks(400, 200, 1000);
        assert!(chunks.len() >= 2 && chunks.len() <= 400);
        assert_eq!(chunks.last().unwrap().1, 400);
    }

    #[test]
    fn handles_zero_pages() {
        assert!(plan_chunks(0, 200, 5).is_empty());
    }
}
