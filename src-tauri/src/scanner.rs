//! 文件枚举 + 分块 SHA256 哈希计算（多线程，全部在 Rust 后端完成）
//!
//! 对应原 Python 脚本逻辑：
//! - 递归遍历文件夹，跳过文件夹本身，只处理文件
//! - 分块读取计算 SHA256（支持超大文件，不整读入内存）
//! - 性能优化：文件大小与另一侧全部文件都不一致时直接跳过哈希

use crate::compare::{ErrorItem, ProgressPayload};
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

/// 哈希分块大小（4MB，防止大文件一次性读入内存）
const CHUNK_SIZE: usize = 4 * 1024 * 1024;

/// 单个文件的处理状态
pub enum EntryStatus {
    /// 尚未处理
    Pending,
    /// 已计算 SHA256
    Hashed(String),
    /// 性能优化跳过：另一侧不存在相同大小的文件，内容必然不同
    SizeSkipped,
    /// 因用户取消而未处理
    Cancelled,
    /// 读取/哈希失败（error 字段记录原因）
    Failed,
}

pub struct ScanEntry {
    pub path: PathBuf,
    pub size: u64,
    pub status: EntryStatus,
    pub error: Option<String>,
}

/// 跨线程共享的进度数据，由独立心跳线程定时推送给前端
pub struct ProgressShared {
    /// "enumerating" | "hashing"
    pub stage: Mutex<&'static str>,
    /// 'A' | 'B'
    pub side: Mutex<char>,
    /// 已枚举 / 已处理数量
    pub scanned: AtomicUsize,
    /// 哈希阶段总数
    pub total: AtomicUsize,
    /// 当前正在处理的文件
    pub current: Mutex<String>,
    /// 结束标志
    pub done: AtomicBool,
}

impl ProgressShared {
    pub fn new() -> Self {
        Self {
            stage: Mutex::new("enumerating"),
            side: Mutex::new('A'),
            scanned: AtomicUsize::new(0),
            total: AtomicUsize::new(0),
            current: Mutex::new(String::new()),
            done: AtomicBool::new(false),
        }
    }

    pub fn payload(&self) -> ProgressPayload {
        let scanned = self.scanned.load(Ordering::Relaxed);
        let total = self.total.load(Ordering::Relaxed);
        ProgressPayload {
            stage: self.stage.lock().unwrap().to_string(),
            side: self.side.lock().unwrap().to_string(),
            scanned,
            total,
            current_file: self.current.lock().unwrap().clone(),
            percent: if total > 0 {
                scanned as f64 / total as f64 * 100.0
            } else {
                -1.0
            },
        }
    }
}

/// 递归枚举文件夹中的全部文件（跳过文件夹本身，只处理文件）
pub fn enumerate_folder(
    root: &Path,
    progress: &ProgressShared,
    cancel: &AtomicBool,
    errors: &Mutex<Vec<ErrorItem>>,
) -> Vec<ScanEntry> {
    let mut entries = Vec::new();
    for item in walkdir::WalkDir::new(root) {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let entry = match item {
            Ok(e) => e,
            Err(e) => {
                let path = e
                    .path()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| root.display().to_string());
                errors.lock().unwrap().push(ErrorItem {
                    path,
                    error: format!("遍历失败: {e}"),
                });
                continue;
            }
        };
        if !entry.file_type().is_file() {
            continue;
        }
        match entry.metadata() {
            Ok(md) => {
                entries.push(ScanEntry {
                    path: entry.into_path(),
                    size: md.len(),
                    status: EntryStatus::Pending,
                    error: None,
                });
                progress.scanned.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => errors.lock().unwrap().push(ErrorItem {
                path: entry.path().display().to_string(),
                error: format!("读取元数据失败: {e}"),
            }),
        }
    }
    entries
}

/// 对需要比对的文件多线程并行计算 SHA256（rayon 线程池 = CPU 逻辑核心数）
pub fn hash_entries(
    entries: &mut [ScanEntry],
    other_sizes: &HashSet<u64>,
    progress: &ProgressShared,
    cancel: &AtomicBool,
    errors: &Mutex<Vec<ErrorItem>>,
) {
    // 性能优化：大小与另一侧全部文件都不一致的，直接跳过哈希（内容必然不同）
    for e in entries.iter_mut() {
        if matches!(e.status, EntryStatus::Pending) && !other_sizes.contains(&e.size) {
            e.status = EntryStatus::SizeSkipped;
        }
    }

    let total = entries
        .iter()
        .filter(|e| matches!(e.status, EntryStatus::Pending))
        .count();
    progress.total.store(total, Ordering::Relaxed);
    progress.scanned.store(0, Ordering::Relaxed);

    let processed = AtomicUsize::new(0);
    entries.par_iter_mut().for_each(|entry| {
        if cancel.load(Ordering::Relaxed) {
            return; // 保持 Pending，稍后标记为 Cancelled
        }
        *progress.current.lock().unwrap() = entry.path.display().to_string();
        match hash_file(&entry.path, cancel) {
            HashOutcome::Done(h) => entry.status = EntryStatus::Hashed(h),
            HashOutcome::Cancelled => {}
            HashOutcome::Failed(msg) => {
                entry.status = EntryStatus::Failed;
                entry.error = Some(msg.clone());
                errors.lock().unwrap().push(ErrorItem {
                    path: entry.path.display().to_string(),
                    error: msg,
                });
            }
        }
        let n = processed.fetch_add(1, Ordering::Relaxed) + 1;
        progress.scanned.store(n, Ordering::Relaxed);
    });

    // 剩余 Pending 的均为取消导致
    for e in entries.iter_mut() {
        if matches!(e.status, EntryStatus::Pending) {
            e.status = EntryStatus::Cancelled;
        }
    }
}

enum HashOutcome {
    Done(String),
    Cancelled,
    Failed(String),
}

/// 分块读取并计算单个文件的 SHA256（支持超大文件；每块检查取消标志）
fn hash_file(path: &Path, cancel: &AtomicBool) -> HashOutcome {
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(e) => return HashOutcome::Failed(format!("打开失败: {e}")),
    };
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; CHUNK_SIZE];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return HashOutcome::Cancelled;
        }
        match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => hasher.update(&buf[..n]),
            Err(e) => return HashOutcome::Failed(format!("读取失败: {e}")),
        }
    }
    HashOutcome::Done(to_hex(&hasher.finalize()))
}

fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}
