//! 全局共享状态：取消标志、运行标志、最近一次比对结果

use crate::compare::CompareResult;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

pub struct Inner {
    /// 用户请求取消
    pub cancel: AtomicBool,
    /// 是否有任务在运行（防止重复启动）
    pub running: AtomicBool,
    /// 最近一次比对结果（CSV 导出用）
    pub last_result: Mutex<Option<CompareResult>>,
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
                last_result: Mutex::new(None),
            }),
        }
    }
}
