//! 单文件夹重复文件扫描：递归枚举 → 按大小分组预筛 → 并行 SHA256 内容确认
//!
//! 与「文件夹比对（compare）」的区别：只扫描一个根目录，在目录内部查找互相重复的文件。
//!
//! 流程：
//! 1. 递归枚举目标文件夹内全部文件（跳过目录与符号链接；遍历 / 读取失败项计入错误列表，不中断任务）
//! 2. 按文件大小分组：大小唯一的文件不可能是重复项，直接跳过哈希（预筛，大文件夹性能关键）
//! 3. 仅对「同大小文件数 ≥ 2」的候选文件多线程并行计算 SHA256（4MB 分块读取，支持超大文件）
//! 4. 按 (大小, 哈希) 分组，组内 ≥ 2 个文件即判定为内容重复，按可释放空间降序输出
//!
//! 进度经 150ms 心跳线程推送前端；取消标志在枚举与每个哈希分块处检查，
//! 中断后仍输出已完成部分的结果并标记 cancelled。

use crate::compare::{ErrorItem, FileInfo};
use crate::scanner::{enumerate_folder, hash_file, HashOutcome, ProgressShared};
use crate::state::Inner;
use rayon::prelude::*;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

/* ---------------- 前端通信类型 ---------------- */

/// 一组内容完全相同的文件（哈希相同，文件名可不同）
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DupGroup {
    /// 组号（从 1 开始，按可释放空间降序排列）
    pub id: usize,
    /// 组内文件统一的 SHA256
    pub hash: String,
    /// 组内每个文件的大小（字节，组内一致）
    pub size: u64,
    /// 组内文件列表（按路径升序，首个为建议保留的副本）
    pub files: Vec<FileInfo>,
    /// 组内仅保留 1 个副本时可释放的空间（字节）
    pub reclaimable: u64,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct DupScanStats {
    /// 枚举到的文件总数
    pub total_files: usize,
    /// 实际计算了哈希的文件数（同大小候选）
    pub hashed: usize,
    /// 大小唯一、无需哈希的文件数（预筛跳过）
    pub skipped_by_size: usize,
    /// 重复组数量
    pub dup_group_count: usize,
    /// 重复文件总数（全部组内文件数之和）
    pub dup_file_count: usize,
    /// 每组保留一个副本后可释放的总空间（字节）
    pub reclaimable_bytes: u64,
    /// 遍历 / 读取 / 哈希失败总数
    pub error_count: usize,
    /// 因取消而未处理的候选文件数
    pub cancelled_count: usize,
    pub duration_ms: u64,
    pub cancelled: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DupScanResult {
    /// 扫描根目录（展示与导出用）
    pub root: String,
    pub groups: Vec<DupGroup>,
    pub errors: Vec<ErrorItem>,
    pub stats: DupScanStats,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DupScanProgressPayload {
    /// "enumerating" | "hashing"
    pub stage: String,
    pub scanned: usize,
    pub total: usize,
    pub current_file: String,
    /// 0-100，-1 表示未知（枚举阶段）
    pub percent: f64,
}

/* ---------------- 扫描主流程 ---------------- */

/// 重复文件扫描主流程（后台线程执行，进度 / 结果通过事件推送，不阻塞调用与 UI）
pub fn run_dup_scan(app: &AppHandle, inner: &Arc<Inner>, root: &Path) -> DupScanResult {
    let start = Instant::now();
    let progress = Arc::new(ProgressShared::new());
    let errors: Mutex<Vec<ErrorItem>> = Mutex::new(Vec::new());

    // 心跳线程：定时把进度推送给前端（不阻塞工作线程）
    {
        let progress = Arc::clone(&progress);
        let app = app.clone();
        std::thread::spawn(move || {
            while !progress.done.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(150));
                if progress.done.load(Ordering::Relaxed) {
                    break;
                }
                let _ = app.emit("dup-scan-progress", dup_payload(&progress));
            }
        });
    }

    // 1. 递归枚举全部文件
    *progress.stage.lock().unwrap() = "enumerating";
    progress.scanned.store(0, Ordering::Relaxed);
    let entries = enumerate_folder(root, &progress, &inner.cancel, &errors);

    // 2. 按大小分组预筛：同大小文件数 ≥ 2 的才进入哈希候选（大小唯一则内容必然不重复）
    let mut by_size: HashMap<u64, Vec<usize>> = HashMap::new();
    for (i, e) in entries.iter().enumerate() {
        by_size.entry(e.size).or_default().push(i);
    }
    let mut candidates: Vec<usize> = by_size
        .values()
        .filter(|v| v.len() >= 2)
        .flatten()
        .copied()
        .collect();
    candidates.sort_unstable();
    let skipped_by_size = entries.len() - candidates.len();

    // 3. 多线程并行哈希（rayon 线程池 = CPU 逻辑核心数；分块读取，每块检查取消标志）
    *progress.stage.lock().unwrap() = "hashing";
    *progress.current.lock().unwrap() = String::new();
    progress.total.store(candidates.len(), Ordering::Relaxed);
    progress.scanned.store(0, Ordering::Relaxed);

    let hashed: Mutex<HashMap<usize, String>> = Mutex::new(HashMap::new());
    let failed = AtomicUsize::new(0);
    let processed = AtomicUsize::new(0);
    candidates.par_iter().for_each(|&i| {
        if inner.cancel.load(Ordering::Relaxed) {
            return; // 不计入失败，稍后统计为「取消未处理」
        }
        *progress.current.lock().unwrap() = entries[i].path.display().to_string();
        match hash_file(&entries[i].path, &inner.cancel) {
            HashOutcome::Done(h) => {
                hashed.lock().unwrap().insert(i, h);
            }
            HashOutcome::Cancelled => {}
            HashOutcome::Failed(msg) => {
                failed.fetch_add(1, Ordering::Relaxed);
                errors.lock().unwrap().push(ErrorItem {
                    path: entries[i].path.display().to_string(),
                    error: msg,
                });
            }
        }
        let n = processed.fetch_add(1, Ordering::Relaxed) + 1;
        progress.scanned.store(n, Ordering::Relaxed);
    });

    // 4. 按 (大小, 哈希) 分组，组内 ≥ 2 个文件即为内容重复
    let hashed = hashed.into_inner().unwrap();
    let hashed_count = hashed.len();
    let mut by_key: HashMap<(u64, String), Vec<usize>> = HashMap::new();
    for (i, h) in hashed.iter() {
        by_key
            .entry((entries[*i].size, h.clone()))
            .or_default()
            .push(*i);
    }

    let mut groups: Vec<DupGroup> = by_key
        .into_iter()
        .filter(|(_, v)| v.len() >= 2)
        .map(|((size, hash), mut idx)| {
            idx.sort_by(|a, b| entries[*a].path.cmp(&entries[*b].path));
            DupGroup {
                id: 0,
                hash,
                size,
                reclaimable: size * (idx.len() as u64 - 1),
                files: idx
                    .iter()
                    .map(|&i| FileInfo {
                        path: entries[i].path.display().to_string(),
                        size: entries[i].size,
                    })
                    .collect(),
            }
        })
        .collect();
    // 可释放空间大的组排前面，便于优先清理
    sort_and_number_groups(&mut groups);

    let dup_file_count: usize = groups.iter().map(|g| g.files.len()).sum();
    let reclaimable_bytes: u64 = groups.iter().map(|g| g.reclaimable).sum();
    let error_count = errors.lock().unwrap().len();
    let cancelled = inner.cancel.load(Ordering::Relaxed);

    let stats = DupScanStats {
        total_files: entries.len(),
        hashed: hashed_count,
        skipped_by_size,
        dup_group_count: groups.len(),
        dup_file_count,
        reclaimable_bytes,
        error_count,
        cancelled_count: candidates
            .len()
            .saturating_sub(hashed_count + failed.load(Ordering::Relaxed)),
        duration_ms: start.elapsed().as_millis() as u64,
        cancelled,
    };

    progress.done.store(true, Ordering::Relaxed);

    DupScanResult {
        root: root.display().to_string(),
        groups,
        errors: errors.into_inner().unwrap(),
        stats,
    }
}

/// 由共享进度数据构建前端进度载荷（与 150ms 心跳线程配合）
fn dup_payload(p: &ProgressShared) -> DupScanProgressPayload {
    let scanned = p.scanned.load(Ordering::Relaxed);
    let total = p.total.load(Ordering::Relaxed);
    DupScanProgressPayload {
        stage: p.stage.lock().unwrap().to_string(),
        scanned,
        total,
        current_file: p.current.lock().unwrap().clone(),
        percent: if total > 0 {
            scanned as f64 / total as f64 * 100.0
        } else {
            -1.0
        },
    }
}

/// 按可释放空间降序重排重复组并重新分配组号（扫描完成 / 删除 / 撤销后共用）
fn sort_and_number_groups(groups: &mut Vec<DupGroup>) {
    groups.sort_by(|a, b| {
        b.reclaimable
            .cmp(&a.reclaimable)
            .then_with(|| b.size.cmp(&a.size))
            .then_with(|| a.hash.cmp(&b.hash))
    });
    for (i, g) in groups.iter_mut().enumerate() {
        g.id = i + 1;
    }
}

/// 从扫描结果中移除已删除的文件：剔除剩余不足 2 个的组，重算各组可释放空间与全局统计
/// （删除主流程与撤销后刷新共用；供 dup_trash 模块调用）
pub(crate) fn apply_deletions(result: &mut DupScanResult, deleted_paths: &[String]) {
    if deleted_paths.is_empty() {
        return;
    }
    let set: HashSet<&str> = deleted_paths.iter().map(String::as_str).collect();
    for g in result.groups.iter_mut() {
        g.files.retain(|f| !set.contains(f.path.as_str()));
        g.reclaimable = g.size * (g.files.len().saturating_sub(1) as u64);
    }
    result.groups.retain(|g| g.files.len() >= 2);
    sort_and_number_groups(&mut result.groups);
    result.stats.dup_group_count = result.groups.len();
    result.stats.dup_file_count = result.groups.iter().map(|g| g.files.len()).sum();
    result.stats.reclaimable_bytes = result.groups.iter().map(|g| g.reclaimable).sum();
}

/// 撤销删除后，把恢复到原位置的文件按 (大小, 哈希) 重新插入重复组：
/// 组仍存在则并入，组已被删光则重建；随后重排组号与统计（与 apply_deletions 互为逆操作）。
/// restored: [(路径, 大小, 哈希)]，哈希为空的条目无法归组，仅跳过不插回。
pub(crate) fn reinsert_restored(result: &mut DupScanResult, restored: &[(String, u64, String)]) {
    for (path, size, hash) in restored {
        if hash.is_empty() {
            continue;
        }
        let f = FileInfo {
            path: path.clone(),
            size: *size,
        };
        match result
            .groups
            .iter_mut()
            .find(|g| g.hash == *hash && g.size == *size)
        {
            Some(g) => {
                if !g.files.iter().any(|x| x.path == *path) {
                    g.files.push(f);
                }
            }
            None => result.groups.push(DupGroup {
                id: 0,
                hash: hash.clone(),
                size: *size,
                files: vec![f],
                reclaimable: 0,
            }),
        }
    }
    for g in result.groups.iter_mut() {
        g.files.sort_by(|a, b| a.path.cmp(&b.path));
        g.reclaimable = g.size * (g.files.len().saturating_sub(1) as u64);
    }
    result.groups.retain(|g| g.files.len() >= 2);
    sort_and_number_groups(&mut result.groups);
    result.stats.dup_group_count = result.groups.len();
    result.stats.dup_file_count = result.groups.iter().map(|g| g.files.len()).sum();
    result.stats.reclaimable_bytes = result.groups.iter().map(|g| g.reclaimable).sum();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, size: u64) -> FileInfo {
        FileInfo {
            path: path.to_string(),
            size,
        }
    }

    fn group(id: usize, hash: &str, size: u64, files: Vec<FileInfo>) -> DupGroup {
        DupGroup {
            id,
            hash: hash.to_string(),
            size,
            reclaimable: size * (files.len() as u64 - 1),
            files,
        }
    }

    fn sample_result() -> DupScanResult {
        DupScanResult {
            root: r"D:\scan".to_string(),
            groups: vec![
                group(
                    1,
                    "aaa",
                    100,
                    vec![file(r"D:\scan\a.bin", 100), file(r"D:\scan\b.bin", 100)],
                ),
                group(
                    2,
                    "bbb",
                    200,
                    vec![
                        file(r"D:\scan\c.bin", 200),
                        file(r"D:\scan\d.bin", 200),
                        file(r"D:\scan\e.bin", 200),
                    ],
                ),
            ],
            errors: vec![],
            stats: DupScanStats {
                total_files: 5,
                hashed: 5,
                skipped_by_size: 0,
                dup_group_count: 2,
                dup_file_count: 5,
                reclaimable_bytes: 500,
                error_count: 0,
                cancelled_count: 0,
                duration_ms: 0,
                cancelled: false,
            },
        }
    }

    /// 删除部分文件后：不足 2 个的组被剔除，可释放空间与统计重算，组号重排
    #[test]
    fn apply_deletions_updates_groups_and_stats() {
        let mut r = sample_result();
        apply_deletions(&mut r, &[r"D:\scan\b.bin".to_string(), r"D:\scan\e.bin".to_string()]);

        // 第 1 组删后仅剩 1 个文件，应被剔除；第 2 组剩 2 个，保留并重算 reclaimable
        assert_eq!(r.groups.len(), 1);
        assert_eq!(r.groups[0].hash, "bbb");
        assert_eq!(r.groups[0].files.len(), 2);
        assert_eq!(r.groups[0].reclaimable, 200);
        assert_eq!(r.groups[0].id, 1); // 重新编号
        assert_eq!(r.stats.dup_group_count, 1);
        assert_eq!(r.stats.dup_file_count, 2);
        assert_eq!(r.stats.reclaimable_bytes, 200);
    }

    /// 空删除列表不改变任何结果
    #[test]
    fn apply_deletions_noop_on_empty() {
        let mut r = sample_result();
        apply_deletions(&mut r, &[]);
        assert_eq!(r.groups.len(), 2);
        assert_eq!(r.stats.reclaimable_bytes, 500);
    }

    /// 撤销删除后：恢复的文件按哈希插回原组，被删光的组重建，统计重算
    #[test]
    fn reinsert_restored_rebuilds_groups_and_stats() {
        let mut r = sample_result();
        // 先删掉第 1 组全部文件与第 2 组一个文件（第 1 组被剔除，第 2 组剩 2 个）
        apply_deletions(
            &mut r,
            &[
                r"D:\scan\a.bin".to_string(),
                r"D:\scan\b.bin".to_string(),
                r"D:\scan\e.bin".to_string(),
            ],
        );
        assert_eq!(r.groups.len(), 1);

        // 撤销：3 个文件恢复原位置
        reinsert_restored(
            &mut r,
            &[
                (r"D:\scan\a.bin".to_string(), 100, "aaa".to_string()),
                (r"D:\scan\b.bin".to_string(), 100, "aaa".to_string()),
                (r"D:\scan\e.bin".to_string(), 200, "bbb".to_string()),
            ],
        );

        assert_eq!(r.groups.len(), 2);
        // aaa 组重建（2 个文件，可释放 100）；bbb 组恢复为 3 个文件，可释放 400
        let aaa = r.groups.iter().find(|g| g.hash == "aaa").unwrap();
        assert_eq!(aaa.files.len(), 2);
        assert_eq!(aaa.reclaimable, 100);
        let bbb = r.groups.iter().find(|g| g.hash == "bbb").unwrap();
        assert_eq!(bbb.files.len(), 3);
        assert_eq!(bbb.reclaimable, 400);
        assert_eq!(r.stats.dup_group_count, 2);
        assert_eq!(r.stats.dup_file_count, 5);
        assert_eq!(r.stats.reclaimable_bytes, 500);
        // 组号按可释放空间降序重排：bbb(400) 应为第 1 组
        assert_eq!(r.groups[0].hash, "bbb");
    }

    /// 哈希未知（空串）的恢复条目无法归组，直接跳过不影响其他条目
    #[test]
    fn reinsert_restored_skips_unknown_hash() {
        let mut r = sample_result();
        reinsert_restored(
            &mut r,
            &[
                (r"D:\scan\x.bin".to_string(), 100, String::new()),
                (r"D:\scan\e.bin".to_string(), 200, "bbb".to_string()),
            ],
        );
        // 第 1 组未受影响；bbb 组并入 e.bin（3 个）
        assert_eq!(r.groups.len(), 2);
        assert_eq!(r.groups.iter().find(|g| g.hash == "aaa").unwrap().files.len(), 2);
        assert_eq!(r.groups.iter().find(|g| g.hash == "bbb").unwrap().files.len(), 3);
    }
}
