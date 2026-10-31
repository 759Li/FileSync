//! 同步操作：比对完成后，将单侧独有文件复制到另一侧；支持撤销
//!
//! 三种动作：
//! - Unify 一键一致化：A 独有 → B 且 B 独有 → A，使两侧内容一致
//! - AToB：仅把 A 独有文件复制到 B
//! - BToA：仅把 B 独有文件复制到 A
//!
//! 安全策略：
//! - 目标侧已存在同名文件（同名不同内容）时跳过并记录，绝不覆盖
//! - 撤销时逐一删除本次会话内复制的目标文件：仅当文件仍存在且大小与复制时一致才删除，
//!   避免误删用户事后修改过的文件；顺带清理因此变空的目录（非空目录不动）

use crate::compare::{CompareResult, ErrorItem, UniqueFile};
use serde::Serialize;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

use crate::state::Inner;

/// 进度事件节流间隔
const EMIT_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SyncAction {
    /// 一键一致化：双向复制
    Unify,
    /// A 独有 → B
    AToB,
    /// B 独有 → A
    BToA,
}

impl SyncAction {
    /// 前端传入的字符串标识 → 动作
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "unify" => Some(Self::Unify),
            "aToB" => Some(Self::AToB),
            "bToA" => Some(Self::BToA),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Unify => "unify",
            Self::AToB => "aToB",
            Self::BToA => "bToA",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Unify => "一键一致化",
            Self::AToB => "A 独有 → B",
            Self::BToA => "B 独有 → A",
        }
    }
}

/* ---------------- 前端通信类型 ---------------- */

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SyncProgressPayload {
    /// 已复制文件数
    pub copied: usize,
    /// 本动作计划复制的文件总数
    pub total: usize,
    /// 当前正在复制的文件
    pub current_file: String,
    /// 0-100
    pub percent: f64,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct SyncResult {
    /// 执行的动作标识
    pub action: String,
    /// 执行的动作名称
    pub action_label: String,
    /// 成功复制数
    pub copied: usize,
    /// 冲突跳过数（目标侧已存在同名文件）
    pub skipped: usize,
    /// 失败数
    pub failed: usize,
    /// 是否被用户取消
    pub cancelled: bool,
    pub duration_ms: u64,
    /// 复制失败明细
    pub errors: Vec<ErrorItem>,
    /// 因冲突而跳过的目标路径
    pub conflicts: Vec<String>,
    /// 本次同步后累计可撤销的复制文件数
    pub undoable: usize,
}

/// 撤销进度事件
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UndoProgressPayload {
    /// 已删除数
    pub removed: usize,
    /// 计划撤销总数
    pub total: usize,
    /// 当前正在撤销的文件
    pub current_file: String,
    /// 0-100
    pub percent: f64,
}

/// 撤销结果事件
#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct UndoResult {
    /// 成功删除（撤销）数
    pub removed: usize,
    /// 目标文件已不存在（可能被手动删除），无需撤销
    pub missing: usize,
    /// 目标文件大小与复制时不一致（可能已被修改），为安全起见保留
    pub modified: usize,
    /// 删除失败数（文件被占用等，仍保留在撤销记录中可重试）
    pub failed: usize,
    /// 是否被用户取消
    pub cancelled: bool,
    /// 顺带清理的空目录数
    pub removed_dirs: usize,
    pub duration_ms: u64,
    /// 撤销后记录中仍可撤销的文件数
    pub remaining: usize,
    /// 删除失败明细
    pub errors: Vec<ErrorItem>,
}

/* ---------------- 复制逻辑 ---------------- */

/// 单个待复制文件（源绝对路径 + 目标绝对路径）
struct CopyItem {
    src: PathBuf,
    dst: PathBuf,
    /// 目标侧根目录（撤销时用于限定空目录清理范围）
    dst_root: PathBuf,
    /// 源文件大小（撤销时校验目标文件未被修改）
    size: u64,
}

/// 一次成功复制的记录（撤销依据），缓存在全局状态中
#[derive(Clone)]
pub struct CopiedItem {
    /// 复制产生的目标文件路径（撤销时删除）
    pub dst: PathBuf,
    /// 目标侧根目录（限定空目录清理范围）
    pub dst_root: PathBuf,
    /// 复制时的文件大小（撤销前校验）
    pub size: u64,
}

/// 执行同步动作，返回统计结果；进度通过 sync-progress 事件推送（节流）
pub fn run_sync(
    app: &AppHandle,
    inner: &Arc<Inner>,
    action: SyncAction,
    result: &CompareResult,
    root_a: &Path,
    root_b: &Path,
) -> SyncResult {
    // 1. 按动作构建复制计划（保持 A→B 在前的顺序）
    let mut plan: Vec<CopyItem> = Vec::new();
    if matches!(action, SyncAction::Unify | SyncAction::AToB) {
        plan.extend(build_items(&result.only_a, root_a, root_b));
    }
    if matches!(action, SyncAction::Unify | SyncAction::BToA) {
        plan.extend(build_items(&result.only_b, root_b, root_a));
    }

    execute_plan(app, inner, action.as_str(), action.label(), plan)
}

/// 一致化勾选的文件：A 侧选中的 → B，B 侧选中的 → A
pub fn run_sync_selected(
    app: &AppHandle,
    inner: &Arc<Inner>,
    selected_a: &[UniqueFile],
    selected_b: &[UniqueFile],
    root_a: &Path,
    root_b: &Path,
) -> SyncResult {
    let mut plan = build_items(selected_a, root_a, root_b);
    plan.extend(build_items(selected_b, root_b, root_a));
    execute_plan(app, inner, "selected", "一致化选中", plan)
}

/// 执行复制计划：串行复制（避免磁盘争抢），进度事件节流推送，成功后累积撤销记录
fn execute_plan(
    app: &AppHandle,
    inner: &Arc<Inner>,
    action: &str,
    action_label: &str,
    plan: Vec<CopyItem>,
) -> SyncResult {
    let start = Instant::now();
    let mut outcome = SyncResult {
        action: action.to_string(),
        action_label: action_label.to_string(),
        ..Default::default()
    };

    let total = plan.len();
    let mut last_emit = Instant::now() - EMIT_INTERVAL; // 保证首个事件立即发出
    let mut copied_items: Vec<CopiedItem> = Vec::new();

    // 2. 逐个复制（串行，避免磁盘争抢；进度事件节流推送）
    for item in &plan {
        if inner.cancel.load(Ordering::Relaxed) {
            outcome.cancelled = true;
            break;
        }
        emit_progress(app, outcome.copied, total, &item.src, &mut last_emit);

        match copy_one(item) {
            CopyOutcome::Copied => {
                outcome.copied += 1;
                copied_items.push(CopiedItem {
                    dst: item.dst.clone(),
                    dst_root: item.dst_root.clone(),
                    size: item.size,
                });
            }
            CopyOutcome::Conflict => {
                outcome.skipped += 1;
                if outcome.conflicts.len() < 500 {
                    outcome.conflicts.push(item.dst.display().to_string());
                }
            }
            CopyOutcome::Failed(err) => {
                outcome.failed += 1;
                if outcome.errors.len() < 500 {
                    outcome.errors.push(ErrorItem {
                        path: item.src.display().to_string(),
                        error: err,
                    });
                }
            }
        }
    }

    // 3. 累积到全局撤销记录（同一比对会话内多次同步可一并撤销）
    {
        let mut record = inner.last_sync.lock().unwrap();
        record.extend(copied_items);
        outcome.undoable = record.len();
    }

    outcome.duration_ms = start.elapsed().as_millis() as u64;
    outcome
}

/// 由「独有文件列表」生成 (源, 目标) 对：保留相对目录结构
fn build_items(files: &[UniqueFile], src_root: &Path, dst_root: &Path) -> Vec<CopyItem> {
    files
        .iter()
        .filter_map(|f| {
            let src = PathBuf::from(&f.path);
            let rel = src.strip_prefix(src_root).ok()?;
            Some(CopyItem {
                dst: dst_root.join(rel),
                dst_root: dst_root.to_path_buf(),
                size: f.size,
                src,
            })
        })
        .collect()
}

enum CopyOutcome {
    Copied,
    /// 目标已存在同名文件，跳过不覆盖
    Conflict,
    Failed(String),
}

fn copy_one(item: &CopyItem) -> CopyOutcome {
    if !item.src.is_file() {
        return CopyOutcome::Failed("源文件不存在（可能已被移动或删除）".into());
    }
    if item.dst.exists() {
        return CopyOutcome::Conflict;
    }
    if let Some(parent) = item.dst.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            return CopyOutcome::Failed(format!("创建目标目录失败: {e}"));
        }
    }
    match fs::copy(&item.src, &item.dst) {
        Ok(_) => CopyOutcome::Copied,
        Err(e) => {
            // 复制失败时清理可能产生的不完整目标文件
            let _ = fs::remove_file(&item.dst);
            CopyOutcome::Failed(format!("复制失败: {e}"))
        }
    }
}

/// 节流推送进度事件（每 EMIT_INTERVAL 最多一条）
fn emit_progress(app: &AppHandle, copied: usize, total: usize, current: &Path, last_emit: &mut Instant) {
    let now = Instant::now();
    if now.duration_since(*last_emit) < EMIT_INTERVAL {
        return;
    }
    *last_emit = now;
    let _ = app.emit(
        "sync-progress",
        SyncProgressPayload {
            copied,
            total,
            current_file: current.display().to_string(),
            percent: if total > 0 {
                copied as f64 / total as f64 * 100.0
            } else {
                0.0
            },
        },
    );
}

/* ---------------- 撤销逻辑 ---------------- */

/// 撤销最近一次（或累计多次）同步：删除复制产生的目标文件，清理空目录。
///
/// 安全规则：
/// - 目标文件不存在 → 视为已撤销（missing）
/// - 目标文件大小 ≠ 复制时记录 → 视为已被修改，保留不删（modified）
/// - 删除失败（占用等）→ 保留在记录中，可再次撤销（failed）
/// - 被取消 → 剩余条目保留在记录中，可继续撤销
pub fn run_undo(app: &AppHandle, inner: &Arc<Inner>, items: Vec<CopiedItem>) -> UndoResult {
    let start = Instant::now();
    let total = items.len();
    let mut outcome = UndoResult::default();
    let mut last_emit = Instant::now() - EMIT_INTERVAL; // 保证首个事件立即发出
    // 已删除文件的祖先目录（用于空目录清理，BTreeSet 便于去重）
    let mut dirty_dirs: BTreeSet<PathBuf> = BTreeSet::new();
    // 仍保留在撤销记录中的条目（删除失败或被取消的）
    let mut kept: Vec<CopiedItem> = Vec::new();

    for (idx, item) in items.iter().enumerate() {
        if inner.cancel.load(Ordering::Relaxed) {
            outcome.cancelled = true;
            kept.extend(items[idx..].iter().cloned());
            break;
        }
        emit_undo_progress(app, outcome.removed, total, &item.dst, &mut last_emit);

        // 大小校验：与复制时不一致说明文件可能被修改过，保留不删
        match fs::metadata(&item.dst) {
            Err(_) => {
                // 目标文件已不存在，无需撤销
                outcome.missing += 1;
            }
            Ok(md) if !md.is_file() || md.len() != item.size => {
                outcome.modified += 1;
            }
            Ok(_) => match fs::remove_file(&item.dst) {
                Ok(()) => {
                    outcome.removed += 1;
                    // 收集 dst_root 之内、逐级向上的祖先目录
                    let mut dir = item.dst.parent().map(Path::to_path_buf);
                    while let Some(d) = dir {
                        if d == item.dst_root || !d.starts_with(&item.dst_root) {
                            break;
                        }
                        let parent = d.parent().map(Path::to_path_buf);
                        dirty_dirs.insert(d);
                        dir = parent;
                    }
                }
                Err(e) => {
                    outcome.failed += 1;
                    if outcome.errors.len() < 500 {
                        outcome.errors.push(ErrorItem {
                            path: item.dst.display().to_string(),
                            error: format!("删除失败: {e}"),
                        });
                    }
                    kept.push(item.clone());
                }
            },
        }
    }

    // 清理因此变空的目录：深度优先（子目录在前），remove_dir 只能删空目录，非空自动失败
    let mut dirs: Vec<PathBuf> = dirty_dirs.into_iter().collect();
    dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
    for d in dirs {
        if fs::remove_dir(&d).is_ok() {
            outcome.removed_dirs += 1;
        }
    }

    // 更新全局撤销记录：仅保留未撤销的条目
    outcome.remaining = kept.len();
    *inner.last_sync.lock().unwrap() = kept;

    outcome.duration_ms = start.elapsed().as_millis() as u64;
    outcome
}

/// 节流推送撤销进度事件
fn emit_undo_progress(app: &AppHandle, removed: usize, total: usize, current: &Path, last_emit: &mut Instant) {
    let now = Instant::now();
    if now.duration_since(*last_emit) < EMIT_INTERVAL {
        return;
    }
    *last_emit = now;
    let _ = app.emit(
        "undo-progress",
        UndoProgressPayload {
            removed,
            total,
            current_file: current.display().to_string(),
            percent: if total > 0 {
                removed as f64 / total as f64 * 100.0
            } else {
                0.0
            },
        },
    );
}
