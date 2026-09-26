//! 比对结果类型定义 + 分类逻辑（完全对标原 Python 脚本的比对规则）
//!
//! 输出 3 类结果：
//! 1. 内容相同（哈希相同，文件名可不同）的重复文件组（A∩B，按哈希分组）
//! 2. 仅文件夹 A 独有的文件
//! 3. 仅文件夹 B 独有的文件

use crate::scanner::{EntryStatus, ScanEntry};
use serde::Serialize;
use std::collections::HashMap;

/* ---------------- 前端通信类型 ---------------- */

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    pub path: String,
    pub size: u64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateGroup {
    pub hash: String,
    pub size: u64,
    pub files_a: Vec<FileInfo>,
    pub files_b: Vec<FileInfo>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UniqueFile {
    pub path: String,
    pub size: u64,
    /// None = 大小优化跳过了哈希（另一侧无同大小文件，必然独有）
    pub hash: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ErrorItem {
    pub path: String,
    pub error: String,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct CompareStats {
    pub total_a: usize,
    pub total_b: usize,
    pub hashed: usize,
    pub skipped_by_size: usize,
    pub error_count: usize,
    pub cancelled_count: usize,
    pub duration_ms: u64,
    pub cancelled: bool,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct CompareResult {
    pub duplicates: Vec<DuplicateGroup>,
    pub only_a: Vec<UniqueFile>,
    pub only_b: Vec<UniqueFile>,
    pub errors: Vec<ErrorItem>,
    pub stats: CompareStats,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProgressPayload {
    pub stage: String,
    pub side: String,
    pub scanned: usize,
    pub total: usize,
    pub current_file: String,
    /// 0-100，-1 表示未知（枚举阶段）
    pub percent: f64,
}

/* ---------------- 分类逻辑 ---------------- */

/// 核心比对：完全沿用 Python 脚本的 A∩B / A-B / B-A 逻辑
pub fn classify(a: &[ScanEntry], b: &[ScanEntry], duration_ms: u64, cancelled: bool) -> CompareResult {
    let mut errors = Vec::new();
    let hashed_a = collect_hashed(a, &mut errors);
    let hashed_b = collect_hashed(b, &mut errors);

    // 第一组：内容重复（哈希交集，按哈希分组）
    let mut duplicates: Vec<DuplicateGroup> = hashed_a
        .iter()
        .filter(|(h, _)| hashed_b.contains_key(*h))
        .map(|(h, fa)| DuplicateGroup {
            hash: h.clone(),
            size: fa[0].size,
            files_a: fa.clone(),
            files_b: hashed_b[h].clone(),
        })
        .collect();
    duplicates.sort_by(|x, y| y.size.cmp(&x.size).then_with(|| x.hash.cmp(&y.hash)));

    // 第二/三组：单侧独有
    let mut only_a = unique_of(a, &hashed_b);
    let mut only_b = unique_of(b, &hashed_a);
    only_a.sort_by(|x, y| x.path.cmp(&y.path));
    only_b.sort_by(|x, y| x.path.cmp(&y.path));

    let stats = CompareStats {
        total_a: a.len(),
        total_b: b.len(),
        hashed: count_status(a, |s| matches!(s, EntryStatus::Hashed(_)))
            + count_status(b, |s| matches!(s, EntryStatus::Hashed(_))),
        skipped_by_size: count_status(a, |s| matches!(s, EntryStatus::SizeSkipped))
            + count_status(b, |s| matches!(s, EntryStatus::SizeSkipped)),
        error_count: errors.len(),
        cancelled_count: count_status(a, |s| matches!(s, EntryStatus::Cancelled))
            + count_status(b, |s| matches!(s, EntryStatus::Cancelled)),
        duration_ms,
        cancelled,
    };

    CompareResult {
        duplicates,
        only_a,
        only_b,
        errors,
        stats,
    }
}

fn collect_hashed(entries: &[ScanEntry], errors: &mut Vec<ErrorItem>) -> HashMap<String, Vec<FileInfo>> {
    let mut map: HashMap<String, Vec<FileInfo>> = HashMap::new();
    for e in entries {
        match &e.status {
            EntryStatus::Hashed(h) => {
                map.entry(h.clone()).or_default().push(FileInfo {
                    path: e.path.display().to_string(),
                    size: e.size,
                });
            }
            EntryStatus::Failed => errors.push(ErrorItem {
                path: e.path.display().to_string(),
                error: e.error.clone().unwrap_or_else(|| "未知错误".into()),
            }),
            _ => {}
        }
    }
    map
}

/// 哈希不在另一侧结果中（或经大小优化判定必然独有）的文件
fn unique_of(entries: &[ScanEntry], other_hashes: &HashMap<String, Vec<FileInfo>>) -> Vec<UniqueFile> {
    entries
        .iter()
        .filter_map(|e| match &e.status {
            EntryStatus::Hashed(h) if !other_hashes.contains_key(h) => Some(UniqueFile {
                path: e.path.display().to_string(),
                size: e.size,
                hash: Some(h.clone()),
            }),
            EntryStatus::SizeSkipped => Some(UniqueFile {
                path: e.path.display().to_string(),
                size: e.size,
                hash: None,
            }),
            _ => None,
        })
        .collect()
}

fn count_status(entries: &[ScanEntry], pred: impl Fn(&EntryStatus) -> bool) -> usize {
    entries.iter().filter(|e| pred(&e.status)).count()
}
