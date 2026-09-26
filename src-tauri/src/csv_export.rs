//! 比对结果导出 CSV（UTF-8 带 BOM，Excel 直接打开中文不乱码）

use crate::compare::CompareResult;
use std::path::Path;

pub fn write_csv(result: &CompareResult, path: &Path) -> Result<(), String> {
    let mut s = String::from('\u{FEFF}'); // BOM

    s.push_str("== 内容重复的文件（A 与 B 内容相同，文件名可能不同） ==\r\n");
    s.push_str("组号,哈希值,文件大小(字节),所属,文件路径\r\n");
    for (i, g) in result.duplicates.iter().enumerate() {
        for f in &g.files_a {
            s.push_str(&format!("{},{},{},A,{}\r\n", i + 1, g.hash, g.size, esc(&f.path)));
        }
        for f in &g.files_b {
            s.push_str(&format!("{},{},{},B,{}\r\n", i + 1, g.hash, g.size, esc(&f.path)));
        }
    }

    s.push_str("\r\n== 仅文件夹 A 独有的文件 ==\r\n");
    s.push_str("文件路径,文件大小(字节),哈希值\r\n");
    for f in &result.only_a {
        s.push_str(&format!(
            "{},{},{}\r\n",
            esc(&f.path),
            f.size,
            f.hash.as_deref().unwrap_or("(大小不同，未计算哈希)")
        ));
    }

    s.push_str("\r\n== 仅文件夹 B 独有的文件 ==\r\n");
    s.push_str("文件路径,文件大小(字节),哈希值\r\n");
    for f in &result.only_b {
        s.push_str(&format!(
            "{},{},{}\r\n",
            esc(&f.path),
            f.size,
            f.hash.as_deref().unwrap_or("(大小不同，未计算哈希)")
        ));
    }

    if !result.errors.is_empty() {
        s.push_str("\r\n== 读取失败的文件 ==\r\n");
        s.push_str("文件路径,错误原因\r\n");
        for e in &result.errors {
            s.push_str(&format!("{},{}\r\n", esc(&e.path), esc(&e.error)));
        }
    }

    s.push_str(&format!(
        "\r\n== 统计 ==\r\nA 文件总数,B 文件总数,计算哈希,大小跳过,读取失败,耗时(毫秒),是否取消\r\n{},{},{},{},{},{},{}\r\n",
        result.stats.total_a,
        result.stats.total_b,
        result.stats.hashed,
        result.stats.skipped_by_size,
        result.stats.error_count,
        result.stats.duration_ms,
        if result.stats.cancelled { "是" } else { "否" }
    ));

    std::fs::write(path, s).map_err(|e| format!("写入 CSV 失败: {e}"))
}

/// CSV 字段转义（含逗号/引号/换行的字段加引号并双写引号）
fn esc(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}
