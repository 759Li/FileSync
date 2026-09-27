//! 全局共享状态：取消标志、运行标志、最近一次比对结果与根目录、同步撤销记录

use crate::compare::CompareResult;
use crate::sync::CopiedItem;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

pub struct Inner {
    /// 用户请求取消
    pub cancel: AtomicBool,
    /// 是否有比对任务在运行（防止重复启动）
    pub running: AtomicBool,
    /// 是否有同步任务在运行（防止与比对/撤销并发）
    pub syncing: AtomicBool,
    /// 是否有撤销任务在运行（防止与比对/同步并发）
    pub undoing: AtomicBool,
    /// 最近一次比对结果（CSV 导出 / 同步操作用）
    pub last_result: Mutex<Option<CompareResult>>,
    /// 最近一次比对的根目录（A, B），同步时用于还原相对路径
    pub last_roots: Mutex<Option<(PathBuf, PathBuf)>>,
    /// 本次比对会话内、历次同步成功复制的文件（撤销依据；新比对开始时清空）
    pub last_sync: Mutex<Vec<CopiedItem>>,
}

#[derive(Clone)]
pub struct CompareState {
    pub inner: Arc<Inner>,
}

impl CompareState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                cancel: AtomicBool::new(false),
                running: AtomicBool::new(false),
                syncing: AtomicBool::new(false),
                undoing: AtomicBool::new(false),
                last_result: Mutex::new(None),
                last_roots: Mutex::new(None),
                last_sync: Mutex::new(Vec::new()),
            }),
        }
    }
}
