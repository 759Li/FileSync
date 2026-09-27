//! 同步操作：比对完成后，将单侧独有文件复制到另一侧
//!
//! 三种动作：
//! - Unify 一键一致化：A 独有 → B 且 B 独有 → A，使两侧内容一致
//! - AToB：仅把 A 独有文件复制到 B
//! - BToA：仅把 B 独有文件复制到 A
//!
//! 安全策略：目标侧已存在同名文件（同名不同内容）时跳过并记录，绝不覆盖。

use crate::compare::{CompareResult, ErrorItem, UniqueFile};
use serde::Serialize;
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
}

/* ---------------- 复制逻辑 ---------------- */

/// 单个待复制文件（源绝对路径 + 目标绝对路径）
struct CopyItem {
    src: PathBuf,
    dst: PathBuf,
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
    let start = Instant::now();
    let mut outcome = SyncResult {
        action: action.as_str().to_string(),
        action_label: action.label().to_string(),
        ..Default::default()
    };

    // 1. 按动作构建复制计划（保持 A→B 在前的顺序）
    let mut plan: Vec<CopyItem> = Vec::new();
    if matches!(action, SyncAction::Unify | SyncAction::AToB) {
        plan.extend(build_items(&result.only_a, root_a, root_b));
    }
    if matches!(action, SyncAction::Unify | SyncAction::BToA) {
        plan.extend(build_items(&result.only_b, root_b, root_a));
    }

    let total = plan.len();
    let mut last_emit = Instant::now() - EMIT_INTERVAL; // 保证首个事件立即发出

    // 2. 逐个复制（串行，避免磁盘争抢；进度事件节流推送）
    for item in &plan {
        if inner.cancel.load(Ordering::Relaxed) {
            outcome.cancelled = true;
            break;
        }
        emit_progress(app, outcome.copied, total, &item.src, &mut last_emit);

        match copy_one(item) {
            CopyOutcome::Copied => outcome.copied += 1,
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
