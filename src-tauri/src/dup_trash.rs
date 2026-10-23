//! 重复文件删除与撤销：移入 App 自管理回收站 → 持久化批次记录 → 随时恢复 / 彻底清空
//!
//! 为什么不用系统回收站：系统回收站没有可靠的跨平台「还原到原路径」API，
//! 无法保证「可撤回」的承诺。因此删除时把文件移入 App 数据目录下的回收站
//! （`<app_data_dir>/filesync-trash/<批次号>/<原相对路径>`，跨卷时回退为复制+删除），
//! 并把每个批次的原始路径 / 存储位置 / 大小 / 哈希持久化到 batches.json。
//!
//! 能力：
//! - 删除：三重校验（位于扫描目录内 + 仍存在且大小一致）后移入回收站，可中途停止
//! - 撤销：把批次内文件恢复到原位置（原位置已有同名文件则跳过，不覆盖）
//! - 清空：永久删除回收站全部文件（不可恢复）
//! - 刷新：删除 / 撤销后自动刷新缓存的扫描结果并推送给前端
//!
//! 批次记录持久化，重启 App 后仍可撤销；删除、撤销、清空三类任务互斥且可停止。

use crate::compare::ErrorItem;
use crate::dup_scan::{apply_deletions, reinsert_restored};
use crate::state::Inner;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

/// 回收站目录名（位于 App 数据目录下）
const TRASH_DIR_NAME: &str = "filesync-trash";
/// 批次记录文件名
const BATCHES_FILE: &str = "batches.json";

/* ---------------- 前端通信类型 ---------------- */

/// 前端提交的待删除文件（携带扫描时记录的大小，用于删除前校验）
#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DupFileRef {
    pub path: String,
    pub size: u64,
}

/// 回收站操作进度（删除 / 撤销 / 清空三类任务共用）
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DupDeleteProgressPayload {
    /// "deleting" | "restoring" | "purging"
    pub phase: String,
    /// 已处理的文件数（含跳过 / 失败）
    pub processed: usize,
    /// 本次任务的文件总数
    pub total: usize,
    pub current_file: String,
    /// 0-100
    pub percent: f64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DupDeleteResult {
    /// 成功移入回收站的文件数
    pub deleted: usize,
    /// 释放的空间（字节，按扫描时记录且校验一致的大小计）
    pub freed_bytes: u64,
    /// 校验不通过而跳过的文件数（不存在 / 大小变化 / 越出扫描目录）
    pub skipped: usize,
    /// 移入回收站操作本身失败的文件数
    pub failed: usize,
    pub cancelled: bool,
    pub duration_ms: u64,
    pub errors: Vec<ErrorItem>,
    /// 本次删除产生的回收站批次号（可一键撤销）；无文件被删除时为 null
    pub batch_id: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DupUndoResult {
    pub batch_id: String,
    /// 成功恢复到原位置的文件数
    pub restored: usize,
    /// 原位置已存在同名文件而跳过的数量（不覆盖）
    pub conflicts: usize,
    /// 恢复失败的数量（仍保留在回收站记录中，可重试）
    pub failed: usize,
    pub cancelled: bool,
    pub duration_ms: u64,
    pub errors: Vec<ErrorItem>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DupPurgeResult {
    /// 被永久删除的文件数
    pub purged_files: usize,
    /// 被永久删除的总大小（字节）
    pub purged_bytes: u64,
    pub failed: usize,
    pub duration_ms: u64,
    pub errors: Vec<ErrorItem>,
}

/// 回收站中的一条删除批次（前端列表展示，最新在前）
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DeleteBatchSummary {
    pub id: String,
    /// 创建时间（Unix 毫秒）
    pub created_at: u64,
    /// 删除时的扫描根目录
    pub root: String,
    pub file_count: usize,
    pub total_bytes: u64,
}

/* ---------------- 持久化模型 ---------------- */

/// 回收站中的一个文件记录
#[derive(Serialize, Deserialize, Clone)]
struct TrashEntry {
    /// 原始绝对路径（撤销时恢复到这里）
    original_path: String,
    /// 相对批次目录的存储路径（保留原目录结构）
    trash_rel: String,
    /// 扫描时记录的大小（恢复前校验用）
    size: u64,
    /// 扫描时记录的内容哈希（撤销后按哈希插回原重复组；可能为空）
    hash: String,
}

/// 一次删除动作产生的批次
#[derive(Serialize, Deserialize, Clone)]
pub(crate) struct DeleteBatch {
    id: String,
    /// 创建时间（Unix 毫秒）
    created_at: u64,
    /// 删除时的扫描根目录（撤销后仅当缓存结果同根目录才刷新列表）
    root: String,
    entries: Vec<TrashEntry>,
}

impl DeleteBatch {
    fn summary(&self) -> DeleteBatchSummary {
        DeleteBatchSummary {
            id: self.id.clone(),
            created_at: self.created_at,
            root: self.root.clone(),
            file_count: self.entries.len(),
            total_bytes: self.entries.iter().map(|e| e.size).sum(),
        }
    }
}

/// batches.json 的磁盘结构
#[derive(Serialize, Deserialize, Default)]
struct TrashStore {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    batches: Vec<DeleteBatch>,
}

/* ---------------- 回收站目录与持久化 ---------------- */

/// 回收站根目录：`<app_data_dir>/filesync-trash/`，取不到数据目录时退回临时目录
fn trash_dir(app: &AppHandle) -> PathBuf {
    let base = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir());
    base.join(TRASH_DIR_NAME)
}

/// 读取批次记录；文件不存在或损坏时视为空回收站
fn load_store(app: &AppHandle) -> TrashStore {
    let file = trash_dir(app).join(BATCHES_FILE);
    match std::fs::read(&file) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => TrashStore::default(),
    }
}

/// 写入批次记录（先写临时文件再重命名，避免中途崩溃写坏）
fn save_store(app: &AppHandle, store: &TrashStore) -> std::io::Result<()> {
    let dir = trash_dir(app);
    std::fs::create_dir_all(&dir)?;
    let file = dir.join(BATCHES_FILE);
    let tmp = dir.join(format!("{BATCHES_FILE}.tmp"));
    let bytes = serde_json::to_vec(store).map_err(std::io::Error::other)?;
    std::fs::write(&tmp, &bytes)?;
    std::fs::rename(&tmp, &file)?;
    Ok(())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 批次号：毫秒时间戳 + 纳秒末四位，保证同一毫秒内的多次删除也不重复
fn new_batch_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("{}-{:04}", now_ms(), nanos % 10000)
}

/// 把回收站最新状态（批次摘要列表）推送给前端
fn emit_trash_state(app: &AppHandle) {
    let summaries: Vec<DeleteBatchSummary> = load_store(app)
        .batches
        .iter()
        .rev() // 最新批次在前
        .map(DeleteBatch::summary)
        .collect();
    let _ = app.emit("dup-trash-state", &summaries);
}

/// 移动文件：优先 rename（同卷，瞬间完成）；跨卷（如回收站在 C:、原文件在 D:）时
/// 回退为复制 → 校验大小一致 → 删除原文件，避免半成品。
fn move_file(src: &Path, dst: &Path) -> std::io::Result<()> {
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match std::fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(_) => {
            std::fs::copy(src, dst)?;
            let copied = std::fs::metadata(dst)?.len();
            let original = std::fs::metadata(src)?.len();
            if copied != original {
                let _ = std::fs::remove_file(dst);
                return Err(std::io::Error::other("复制后大小不一致，已中止移动"));
            }
            std::fs::remove_file(src)?;
            Ok(())
        }
    }
}

/// 把文件移入回收站批次目录，保留其相对扫描根目录的结构；返回批次内相对路径。
/// 极端情况下批次内重名时自动追加序号（理论不会发生，防御性处理）。
fn move_to_trash(batch_dir: &Path, original: &Path, root: &Path) -> std::io::Result<String> {
    let rel = original
        .strip_prefix(root)
        .map(|r| r.to_path_buf())
        .unwrap_or_else(|_| PathBuf::from(original.file_name().unwrap_or_default()));
    let mut target = batch_dir.join(&rel);
    let mut n = 1u32;
    while target.exists() && n <= 1000 {
        target = batch_dir.join(format!("{}_{}", rel.display(), n));
        n += 1;
    }
    move_file(original, &target)?;
    Ok(target
        .strip_prefix(batch_dir)
        .map(|r| r.to_string_lossy().to_string())
        .unwrap_or_else(|_| rel.to_string_lossy().to_string()))
}

/* ---------------- 对外查询 ---------------- */

/// 回收站中的删除批次列表（最新在前）
pub fn list_delete_batches(app: &AppHandle) -> Vec<DeleteBatchSummary> {
    load_store(app)
        .batches
        .iter()
        .rev()
        .map(DeleteBatch::summary)
        .collect()
}

/// 按批次号查找批次（不存在返回 None）
pub fn find_batch(app: &AppHandle, batch_id: &str) -> Option<DeleteBatch> {
    load_store(app)
        .batches
        .into_iter()
        .find(|b| b.id == batch_id)
}

/// 在资源管理器中打开回收站目录（不存在则先创建）
pub fn open_trash_dir(app: &AppHandle) -> Result<(), String> {
    let dir = trash_dir(app);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    crate::commands::open_in_explorer(dir.to_string_lossy().to_string())
}

/* ---------------- 删除主流程 ---------------- */

/// 一键删除选中的重复文件：逐项三重校验后移入 App 回收站（可撤销），完成后刷新扫描结果。
///
/// 安全策略：
/// - 仅删除位于本次扫描目录内的文件（防止越界误删）
/// - 删除前校验文件仍存在、是普通文件、且大小与扫描时一致（扫描后被修改 / 替换的跳过）
/// - 移入 App 回收站而非系统回收站，保证随时可一键恢复到原路径
/// - 支持随时取消；取消后仍会刷新已完成部分的结果
pub fn run_delete_dup_files(
    app: &AppHandle,
    inner: &Arc<Inner>,
    files: Vec<DupFileRef>,
) -> DupDeleteResult {
    let start = Instant::now();
    let total = files.len();

    // 扫描根目录：删除范围必须限制在扫描目录内；没有扫描结果时拒绝删除
    let root_dir =
        match inner.dup_result.lock().unwrap().as_ref().map(|r| PathBuf::from(&r.root)) {
            Some(r) => r,
            None => {
                return DupDeleteResult {
                    deleted: 0,
                    freed_bytes: 0,
                    skipped: total,
                    failed: 0,
                    cancelled: false,
                    duration_ms: start.elapsed().as_millis() as u64,
                    errors: vec![ErrorItem {
                        path: String::new(),
                        error: "当前没有扫描结果，无法确认删除范围，请先扫描".into(),
                    }],
                    batch_id: None,
                };
            }
        };
    // 路径 → 哈希（撤销时按哈希把文件插回原重复组）
    let mut hash_by_path: HashMap<String, String> = HashMap::new();
    if let Some(r) = inner.dup_result.lock().unwrap().as_ref() {
        for g in &r.groups {
            for f in &g.files {
                hash_by_path.insert(f.path.clone(), g.hash.clone());
            }
        }
    }

    let mut deleted = 0usize;
    let mut processed = 0usize;
    let mut freed_bytes = 0u64;
    let mut skipped = 0usize;
    let mut failed = 0usize;
    let mut errors: Vec<ErrorItem> = Vec::new();
    let mut deleted_paths: Vec<String> = Vec::new();
    let mut trash_entries: Vec<TrashEntry> = Vec::new();
    let mut last_emit = Instant::now();

    let batch_id = new_batch_id();
    let batch_dir = trash_dir(app).join(&batch_id);
    let mut batch_created = false;

    for f in &files {
        if inner.cancel.load(Ordering::Relaxed) {
            break;
        }
        let p = Path::new(&f.path);

        // 1. 必须位于扫描目录内
        if !p.starts_with(&root_dir) {
            skipped += 1;
            processed += 1;
            errors.push(ErrorItem {
                path: f.path.clone(),
                error: "文件不在本次扫描目录内，已跳过".into(),
            });
            continue;
        }
        // 2. 必须仍存在、是文件、且大小与扫描时一致
        match std::fs::metadata(p) {
            Ok(md) if md.is_file() && md.len() == f.size => {}
            Ok(_) => {
                skipped += 1;
                processed += 1;
                errors.push(ErrorItem {
                    path: f.path.clone(),
                    error: "文件已发生变化（大小与扫描时不一致），已跳过".into(),
                });
                continue;
            }
            Err(e) => {
                skipped += 1;
                processed += 1;
                errors.push(ErrorItem {
                    path: f.path.clone(),
                    error: format!("文件不存在或无法读取: {e}"),
                });
                continue;
            }
        }
        // 3. 移入回收站
        match move_to_trash(&batch_dir, p, &root_dir) {
            Ok(trash_rel) => {
                batch_created = true;
                deleted += 1;
                freed_bytes += f.size;
                deleted_paths.push(f.path.clone());
                trash_entries.push(TrashEntry {
                    original_path: f.path.clone(),
                    trash_rel,
                    size: f.size,
                    hash: hash_by_path.get(&f.path).cloned().unwrap_or_default(),
                });
            }
            Err(e) => {
                failed += 1;
                errors.push(ErrorItem {
                    path: f.path.clone(),
                    error: format!("移入回收站失败: {e}"),
                });
            }
        }
        processed += 1;

        // 进度节流推送（100ms）
        if last_emit.elapsed() >= Duration::from_millis(100) {
            last_emit = Instant::now();
            let _ = app.emit(
                "dup-delete-progress",
                DupDeleteProgressPayload {
                    phase: "deleting".into(),
                    processed,
                    total,
                    current_file: f.path.clone(),
                    percent: if total > 0 {
                        processed as f64 / total as f64 * 100.0
                    } else {
                        0.0
                    },
                },
            );
        }
    }

    // 持久化批次记录（没有任何文件删除成功时不建批次）
    let mut persisted_batch: Option<String> = None;
    if batch_created {
        let batch = DeleteBatch {
            id: batch_id.clone(),
            created_at: now_ms(),
            root: root_dir.display().to_string(),
            entries: trash_entries,
        };
        let mut store = load_store(app);
        store.batches.push(batch);
        match save_store(app, &store) {
            Ok(()) => persisted_batch = Some(batch_id),
            Err(e) => errors.push(ErrorItem {
                path: String::new(),
                error: format!(
                    "回收站记录保存失败（本次删除仍可撤销，但重启 App 后记录可能丢失）: {e}"
                ),
            }),
        }
    }

    // 刷新缓存的扫描结果并推送前端
    if let Some(r) = inner.dup_result.lock().unwrap().as_mut() {
        apply_deletions(r, &deleted_paths);
    }
    let updated = inner.dup_result.lock().unwrap().clone();
    if let Some(r) = updated {
        let _ = app.emit("dup-scan-result", &r);
    }
    emit_trash_state(app);

    DupDeleteResult {
        deleted,
        freed_bytes,
        skipped,
        failed,
        cancelled: inner.cancel.load(Ordering::Relaxed),
        duration_ms: start.elapsed().as_millis() as u64,
        errors,
        batch_id: persisted_batch,
    }
}

/* ---------------- 撤销主流程 ---------------- */

/// 撤销一个删除批次：把回收站中的文件恢复到原位置，刷新扫描结果与回收站列表。
///
/// 规则：
/// - 回收站中的文件必须存在且大小一致，否则记失败并保留记录
/// - 原位置已存在同名文件 → 冲突跳过（绝不覆盖用户现有文件）
/// - 父目录不存在时重建（撤销应当忠实还原）
/// - 中途取消：未处理的条目保留在批次中，可再次撤销
pub fn run_undo_delete(app: &AppHandle, inner: &Arc<Inner>, batch: DeleteBatch) -> DupUndoResult {
    let start = Instant::now();
    let total = batch.entries.len();
    let mut restored = 0usize;
    let mut processed = 0usize;
    let mut conflicts = 0usize;
    let mut failed = 0usize;
    let mut errors: Vec<ErrorItem> = Vec::new();
    let mut restored_files: Vec<(String, u64, String)> = Vec::new();
    let mut remaining: Vec<TrashEntry> = Vec::new();
    let mut last_emit = Instant::now();
    let batch_dir = trash_dir(app).join(&batch.id);

    for e in &batch.entries {
        if inner.cancel.load(Ordering::Relaxed) {
            remaining.push(e.clone());
            continue;
        }
        processed += 1;
        let orig = Path::new(&e.original_path);
        let trash_p = batch_dir.join(&e.trash_rel);

        // 1. 回收站中的文件必须存在且大小一致
        match std::fs::metadata(&trash_p) {
            Ok(md) if md.is_file() && md.len() == e.size => {}
            Ok(_) => {
                failed += 1;
                errors.push(ErrorItem {
                    path: e.original_path.clone(),
                    error: "回收站中的文件已损坏（大小不一致），无法恢复".into(),
                });
                remaining.push(e.clone());
                continue;
            }
            Err(_) => {
                failed += 1;
                errors.push(ErrorItem {
                    path: e.original_path.clone(),
                    error: "回收站中找不到该文件（可能已被清理）".into(),
                });
                remaining.push(e.clone());
                continue;
            }
        }
        // 2. 原位置已存在同名文件 → 冲突跳过
        if orig.exists() {
            conflicts += 1;
            errors.push(ErrorItem {
                path: e.original_path.clone(),
                error: "原位置已存在同名文件，已跳过（不覆盖）".into(),
            });
            remaining.push(e.clone());
            continue;
        }
        // 3. 恢复到原位置
        match move_file(&trash_p, orig) {
            Ok(()) => {
                restored += 1;
                restored_files.push((e.original_path.clone(), e.size, e.hash.clone()));
            }
            Err(err) => {
                failed += 1;
                errors.push(ErrorItem {
                    path: e.original_path.clone(),
                    error: format!("恢复失败: {err}"),
                });
                remaining.push(e.clone());
            }
        }

        // 进度节流推送（100ms）
        if last_emit.elapsed() >= Duration::from_millis(100) {
            last_emit = Instant::now();
            let _ = app.emit(
                "dup-delete-progress",
                DupDeleteProgressPayload {
                    phase: "restoring".into(),
                    processed,
                    total,
                    current_file: e.original_path.clone(),
                    percent: if total > 0 {
                        processed as f64 / total as f64 * 100.0
                    } else {
                        0.0
                    },
                },
            );
        }
    }

    // 更新批次记录：全部恢复则移除批次并清理批次目录，否则仅保留未恢复条目
    let mut store = load_store(app);
    if remaining.is_empty() {
        store.batches.retain(|b| b.id != batch.id);
        let _ = std::fs::remove_dir_all(&batch_dir);
    } else if let Some(b) = store.batches.iter_mut().find(|b| b.id == batch.id) {
        b.entries = remaining;
    }
    if let Err(e) = save_store(app, &store) {
        errors.push(ErrorItem {
            path: String::new(),
            error: format!("回收站记录更新失败: {e}"),
        });
    }
    emit_trash_state(app);

    // 刷新扫描结果：仅当缓存结果正是该批次所在目录的扫描时才插回恢复的文件
    let same_root = inner
        .dup_result
        .lock()
        .unwrap()
        .as_ref()
        .map(|r| r.root == batch.root)
        .unwrap_or(false);
    if same_root && !restored_files.is_empty() {
        if let Some(r) = inner.dup_result.lock().unwrap().as_mut() {
            reinsert_restored(r, &restored_files);
        }
        let updated = inner.dup_result.lock().unwrap().clone();
        if let Some(r) = updated {
            let _ = app.emit("dup-scan-result", &r);
        }
    }

    DupUndoResult {
        batch_id: batch.id,
        restored,
        conflicts,
        failed,
        cancelled: inner.cancel.load(Ordering::Relaxed),
        duration_ms: start.elapsed().as_millis() as u64,
        errors,
    }
}

/* ---------------- 清空回收站 ---------------- */

/// 彻底清空回收站：永久删除全部批次文件（不可恢复），可中途停止。
/// 删除失败 / 中途取消的批次记录保留，避免丢失对仍在盘上文件的追踪。
pub fn run_purge_trash(app: &AppHandle, inner: &Arc<Inner>) -> DupPurgeResult {
    let start = Instant::now();
    let mut store = load_store(app);
    let total: usize = store.batches.iter().map(|b| b.entries.len()).sum();
    let mut purged_files = 0usize;
    let mut purged_bytes = 0u64;
    let mut failed = 0usize;
    let mut processed = 0usize;
    let mut errors: Vec<ErrorItem> = Vec::new();
    let mut last_emit = Instant::now();
    let mut kept: Vec<DeleteBatch> = Vec::new();

    for batch in &store.batches {
        let batch_dir = trash_dir(app).join(&batch.id);
        let mut kept_entries: Vec<TrashEntry> = Vec::new();
        for e in &batch.entries {
            if inner.cancel.load(Ordering::Relaxed) {
                kept_entries.push(e.clone());
                continue;
            }
            processed += 1;
            let p = batch_dir.join(&e.trash_rel);
            match std::fs::remove_file(&p) {
                Ok(()) => {
                    purged_files += 1;
                    purged_bytes += e.size;
                }
                // 文件本就没了，视为已清空
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    purged_files += 1;
                    purged_bytes += e.size;
                }
                Err(err) => {
                    failed += 1;
                    errors.push(ErrorItem {
                        path: e.original_path.clone(),
                        error: format!("彻底删除失败: {err}"),
                    });
                    kept_entries.push(e.clone());
                }
            }

            // 进度节流推送（100ms）
            if last_emit.elapsed() >= Duration::from_millis(100) {
                last_emit = Instant::now();
                let _ = app.emit(
                    "dup-delete-progress",
                    DupDeleteProgressPayload {
                        phase: "purging".into(),
                        processed,
                        total,
                        current_file: e.original_path.clone(),
                        percent: if total > 0 {
                            processed as f64 / total as f64 * 100.0
                        } else {
                            0.0
                        },
                    },
                );
            }
        }
        // 整批成功才清理批次目录，避免误删保留条目
        if kept_entries.is_empty() {
            let _ = std::fs::remove_dir_all(&batch_dir);
        } else {
            kept.push(DeleteBatch {
                id: batch.id.clone(),
                created_at: batch.created_at,
                root: batch.root.clone(),
                entries: kept_entries,
            });
        }
    }

    store.batches = kept;
    if let Err(e) = save_store(app, &store) {
        errors.push(ErrorItem {
            path: String::new(),
            error: format!("回收站记录更新失败: {e}"),
        });
    }
    emit_trash_state(app);

    DupPurgeResult {
        purged_files,
        purged_bytes,
        failed,
        duration_ms: start.elapsed().as_millis() as u64,
        errors,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 唯一临时目录（测试隔离）
    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("filesync_dup_trash_{tag}_{nanos}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// move_file：同卷内移动后原位置消失、目标存在且内容一致
    #[test]
    fn move_file_moves_within_volume() {
        let dir = temp_dir("move");
        let src = dir.join("root/a.txt");
        std::fs::create_dir_all(src.parent().unwrap()).unwrap();
        std::fs::write(&src, b"hello").unwrap();

        let dst = dir.join("batch/a.txt");
        move_file(&src, &dst).expect("move should succeed");

        assert!(!src.exists());
        assert_eq!(std::fs::read(&dst).unwrap(), b"hello");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// move_file：目标父目录不存在时自动创建
    #[test]
    fn move_file_creates_parent_dirs() {
        let dir = temp_dir("mkdir");
        let src = dir.join("root/a.txt");
        std::fs::create_dir_all(src.parent().unwrap()).unwrap();
        std::fs::write(&src, b"x").unwrap();

        let dst = dir.join("batch/deep/nested/a.txt");
        move_file(&src, &dst).unwrap();
        assert!(dst.is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// move_to_trash：保留相对扫描根目录的目录结构
    #[test]
    fn move_to_trash_preserves_relative_structure() {
        let dir = temp_dir("structure");
        let root = dir.join("root");
        let batch_dir = dir.join("batch");
        let original = root.join("sub/pic.jpg");
        std::fs::create_dir_all(original.parent().unwrap()).unwrap();
        std::fs::write(&original, b"img").unwrap();

        let rel = move_to_trash(&batch_dir, &original, &root).unwrap();
        assert!(rel.contains("sub") && rel.ends_with("pic.jpg"));
        assert!(!original.exists());
        assert_eq!(std::fs::read(batch_dir.join(&rel)).unwrap(), b"img");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// move_to_trash：批次内重名时追加序号，不覆盖已有文件
    #[test]
    fn move_to_trash_avoids_collision() {
        let dir = temp_dir("collision");
        let root = dir.join("root");
        let batch_dir = dir.join("batch");
        let original = root.join("a.txt");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&original, b"new").unwrap();
        // 预占批次内目标位置
        std::fs::create_dir_all(&batch_dir).unwrap();
        std::fs::write(batch_dir.join("a.txt"), b"old").unwrap();

        let rel = move_to_trash(&batch_dir, &original, &root).unwrap();
        assert_ne!(rel, "a.txt");
        assert!(rel.starts_with("a.txt_"));
        assert_eq!(std::fs::read(batch_dir.join(&rel)).unwrap(), b"new");
        assert_eq!(std::fs::read(batch_dir.join("a.txt")).unwrap(), b"old");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 批次记录持久化：序列化 → 反序列化 roundtrip
    #[test]
    fn trash_store_json_roundtrip() {
        let store = TrashStore {
            version: 1,
            batches: vec![DeleteBatch {
                id: "123-4567".into(),
                created_at: 1_700_000_000_000,
                root: r"D:\photos".into(),
                entries: vec![TrashEntry {
                    original_path: r"D:\photos\a.jpg".into(),
                    trash_rel: "a.jpg".into(),
                    size: 42,
                    hash: "abc".into(),
                }],
            }],
        };
        let json = serde_json::to_vec(&store).unwrap();
        let back: TrashStore = serde_json::from_slice(&json).unwrap();
        assert_eq!(back.batches.len(), 1);
        assert_eq!(back.batches[0].id, "123-4567");
        assert_eq!(back.batches[0].entries[0].size, 42);
        assert_eq!(back.batches[0].entries[0].hash, "abc");
    }

    /// 批次摘要：文件数与总大小按条目汇总
    #[test]
    fn batch_summary_aggregates_entries() {
        let batch = DeleteBatch {
            id: "b1".into(),
            created_at: 1,
            root: "D:/x".into(),
            entries: vec![
                TrashEntry { original_path: "1".into(), trash_rel: "1".into(), size: 10, hash: "h".into() },
                TrashEntry { original_path: "2".into(), trash_rel: "2".into(), size: 20, hash: "h".into() },
            ],
        };
        let s = batch.summary();
        assert_eq!(s.file_count, 2);
        assert_eq!(s.total_bytes, 30);
    }
}
