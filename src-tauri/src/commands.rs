//! Tauri 命令：启动/停止比对、同步操作、导出 CSV、打开文件所在位置

use crate::compare::{classify, CompareResult, ErrorItem};
use crate::scanner::{enumerate_folder, hash_entries, ProgressShared};
use crate::state::{CompareState, Inner};
use crate::sync::{run_sync, SyncAction};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, State};

/// 启动比对。后台线程执行，进度/结果通过事件推送，不阻塞调用与 UI。
#[tauri::command]
pub fn start_compare(
    app: AppHandle,
    state: State<'_, CompareState>,
    path_a: String,
    path_b: String,
) -> Result<(), String> {
    let dir_a = PathBuf::from(&path_a);
    let dir_b = PathBuf::from(&path_b);
    if !dir_a.is_dir() {
        return Err(format!("文件夹A路径无效: {path_a}"));
    }
    if !dir_b.is_dir() {
        return Err(format!("文件夹B路径无效: {path_b}"));
    }
    if state.inner.running.swap(true, Ordering::SeqCst) {
        return Err("已有比对任务正在进行".into());
    }
    if state.inner.syncing.load(Ordering::SeqCst) || state.inner.undoing.load(Ordering::SeqCst) {
        state.inner.running.store(false, Ordering::SeqCst);
        return Err("同步/撤销任务正在进行，请等待完成或停止后再比对".into());
    }
    state.inner.cancel.store(false, Ordering::SeqCst);
    // 记录根目录，供比对后的同步操作还原相对路径；清空上一会话的撤销记录
    *state.inner.last_roots.lock().unwrap() = Some((dir_a.clone(), dir_b.clone()));
    state.inner.last_sync.lock().unwrap().clear();

    let inner = state.inner.clone();
    std::thread::spawn(move || {
        let result = run_compare(&app, &inner, &dir_a, &dir_b);
        *inner.last_result.lock().unwrap() = Some(result.clone());
        inner.running.store(false, Ordering::SeqCst);
        let _ = app.emit("compare-result", &result);
    });
    Ok(())
}

/// 终止正在运行的扫描任务
#[tauri::command]
pub fn stop_compare(state: State<'_, CompareState>) -> bool {
    state.inner.cancel.store(true, Ordering::SeqCst);
    state.inner.running.load(Ordering::SeqCst)
}

/// 执行同步动作（基于最近一次完整比对结果，后台线程复制文件）
///
/// action: "unify" 一键一致化 | "aToB" A独有→B | "bToA" B独有→A
#[tauri::command]
pub fn apply_sync_action(
    app: AppHandle,
    state: State<'_, CompareState>,
    action: String,
) -> Result<(), String> {
    let action = SyncAction::parse(&action)
        .ok_or_else(|| format!("未知的同步动作: {action}（可选 unify / aToB / bToA）"))?;

    if state.inner.running.load(Ordering::SeqCst) {
        return Err("比对任务正在进行，请等待完成或停止后再同步".into());
    }
    if state.inner.undoing.load(Ordering::SeqCst) {
        return Err("撤销任务正在进行，请等待完成或停止后再同步".into());
    }
    if state.inner.syncing.swap(true, Ordering::SeqCst) {
        return Err("已有同步任务正在进行".into());
    }

    let result = state.inner.last_result.lock().unwrap().clone();
    let roots = state.inner.last_roots.lock().unwrap().clone();
    let inner = state.inner.clone();

    let (result, (dir_a, dir_b)) = match (result, roots) {
        (Some(r), Some(p)) => (r, p),
        _ => {
            state.inner.syncing.store(false, Ordering::SeqCst);
            return Err("当前没有比对结果，请先执行比对".into());
        }
    };
    if result.stats.cancelled {
        state.inner.syncing.store(false, Ordering::SeqCst);
        return Err("上次比对被中断、结果不完整，请重新完成比对后再同步".into());
    }
    if !dir_a.is_dir() || !dir_b.is_dir() {
        state.inner.syncing.store(false, Ordering::SeqCst);
        return Err("比对时的某个文件夹已不存在，无法执行同步".into());
    }

    state.inner.cancel.store(false, Ordering::SeqCst);
    std::thread::spawn(move || {
        let sync_result = run_sync(&app, &inner, action, &result, &dir_a, &dir_b);
        inner.syncing.store(false, Ordering::SeqCst);
        let _ = app.emit("sync-result", &sync_result);
    });
    Ok(())
}

/// 终止正在运行的同步任务
#[tauri::command]
pub fn stop_sync(state: State<'_, CompareState>) -> bool {
    state.inner.cancel.store(true, Ordering::SeqCst);
    state.inner.syncing.load(Ordering::SeqCst)
}

/// 撤销本次比对会话内的同步操作：删除复制产生的目标文件（后台线程执行）
///
/// 安全规则：目标文件已不存在视为已撤销；大小与复制时不一致视为已被修改、保留不删；
/// 删除失败或中途停止的条目保留在记录中，可再次执行撤销继续。
#[tauri::command]
pub fn undo_sync(app: AppHandle, state: State<'_, CompareState>) -> Result<(), String> {
    if state.inner.running.load(Ordering::SeqCst) {
        return Err("比对任务正在进行，请等待完成或停止后再撤销".into());
    }
    if state.inner.syncing.load(Ordering::SeqCst) {
        return Err("同步任务正在进行，请等待完成或停止后再撤销".into());
    }
    if state.inner.undoing.swap(true, Ordering::SeqCst) {
        return Err("已有撤销任务正在进行".into());
    }

    let items = state.inner.last_sync.lock().unwrap().clone();
    if items.is_empty() {
        state.inner.undoing.store(false, Ordering::SeqCst);
        return Err("当前没有可撤销的同步操作".into());
    }

    let inner = state.inner.clone();
    state.inner.cancel.store(false, Ordering::SeqCst);
    std::thread::spawn(move || {
        let undo_result = crate::sync::run_undo(&app, &inner, items);
        inner.undoing.store(false, Ordering::SeqCst);
        let _ = app.emit("undo-result", &undo_result);
    });
    Ok(())
}

/// 终止正在运行的撤销任务（未完成的条目保留在记录中，可继续撤销）
#[tauri::command]
pub fn stop_undo(state: State<'_, CompareState>) -> bool {
    state.inner.cancel.store(true, Ordering::SeqCst);
    state.inner.undoing.load(Ordering::SeqCst)
}

/// 导出最近一次比对结果为 CSV（后端缓存的完整结果，UTF-8 BOM，Excel 打开不乱码）
#[tauri::command]
pub fn export_csv(state: State<'_, CompareState>, save_path: String) -> Result<String, String> {
    let result = state.inner.last_result.lock().unwrap().clone();
    match result {
        Some(r) => {
            crate::csv_export::write_csv(&r, Path::new(&save_path))?;
            Ok(save_path)
        }
        None => Err("当前没有比对结果，请先执行比对".into()),
    }
}

/// 在资源管理器中打开文件所在文件夹（并选中该文件）
#[tauri::command]
pub fn open_in_explorer(path: String) -> Result<(), String> {
    let p = PathBuf::from(&path);
    if p.is_file() {
        std::process::Command::new("explorer")
            .arg(format!("/select,{}", p.display()))
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    } else if p.is_dir() {
        std::process::Command::new("explorer")
            .arg(&p)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    } else {
        Err(format!("路径不存在: {path}"))
    }
}

/// 比对主流程：枚举 A/B → 大小优化 → 并行哈希 → 分类
fn run_compare(app: &AppHandle, inner: &Arc<Inner>, dir_a: &Path, dir_b: &Path) -> CompareResult {
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
                let _ = app.emit("compare-progress", progress.payload());
            }
        });
    }

    // 1. 递归枚举两个文件夹
    *progress.stage.lock().unwrap() = "enumerating";
    *progress.side.lock().unwrap() = 'A';
    progress.scanned.store(0, Ordering::Relaxed);
    let mut entries_a = enumerate_folder(dir_a, &progress, &inner.cancel, &errors);

    *progress.side.lock().unwrap() = 'B';
    progress.scanned.store(0, Ordering::Relaxed);
    let mut entries_b = enumerate_folder(dir_b, &progress, &inner.cancel, &errors);

    // 2. 收集两侧文件大小集合（性能优化依据）
    let sizes_a: HashSet<u64> = entries_a.iter().map(|e| e.size).collect();
    let sizes_b: HashSet<u64> = entries_b.iter().map(|e| e.size).collect();

    // 3. 多线程并行哈希（仅对大小可能匹配的文件计算）
    *progress.stage.lock().unwrap() = "hashing";
    *progress.side.lock().unwrap() = 'A';
    *progress.current.lock().unwrap() = String::new();
    hash_entries(&mut entries_a, &sizes_b, &progress, &inner.cancel, &errors);

    *progress.side.lock().unwrap() = 'B';
    *progress.current.lock().unwrap() = String::new();
    hash_entries(&mut entries_b, &sizes_a, &progress, &inner.cancel, &errors);

    // 4. 分类输出 3 组结果
    let cancelled = inner.cancel.load(Ordering::Relaxed);
    let result = classify(
        &entries_a,
        &entries_b,
        start.elapsed().as_millis() as u64,
        cancelled,
    );

    progress.done.store(true, Ordering::Relaxed);
    result
}
