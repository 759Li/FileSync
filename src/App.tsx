import { useEffect, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { save } from "@tauri-apps/plugin-dialog";
import { Button } from "./components/ui/button";
import { FolderPicker } from "./components/FolderPicker";
import { ProgressPanel } from "./components/ProgressPanel";
import { ResultsView } from "./components/ResultsView";
import type { CompareResult, ProgressPayload } from "./types";
import { formatDuration } from "./lib/format";

export default function App(): ReactNode {
  const [pathA, setPathA] = useState("");
  const [pathB, setPathB] = useState("");
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState<ProgressPayload | null>(null);
  const [result, setResult] = useState<CompareResult | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [messageType, setMessageType] = useState<"info" | "error">("info");

  /* 订阅后端事件：进度 + 最终结果 */
  useEffect(() => {
    const u1 = listen<ProgressPayload>("compare-progress", (e) => setProgress(e.payload));
    const u2 = listen<CompareResult>("compare-result", (e) => {
      setResult(e.payload);
      setRunning(false);
      setProgress(null);
    });
    return () => {
      u1.then((f: UnlistenFn) => f());
      u2.then((f: UnlistenFn) => f());
    };
  }, []);

  const notify = (text: string, type: "info" | "error" = "info") => {
    setMessage(text);
    setMessageType(type);
  };

  const handleStart = async () => {
    setMessage(null);
    setResult(null);
    setProgress(null);
    try {
      await invoke("start_compare", { pathA, pathB });
      setRunning(true);
    } catch (e) {
      notify(String(e), "error");
    }
  };

  const handleStop = () => {
    invoke("stop_compare").catch((e) => notify(String(e), "error"));
  };

  const handleExport = async () => {
    const p = await save({
      title: "导出比对结果 CSV",
      defaultPath: "比对结果.csv",
      filters: [{ name: "CSV 文件", extensions: ["csv"] }],
    });
    if (!p) return;
    try {
      await invoke("export_csv", { savePath: p });
      notify(`已导出到：${p}`);
    } catch (e) {
      notify(String(e), "error");
    }
  };

  const s = result?.stats;

  return (
    <div className="mx-auto min-h-screen w-full max-w-5xl space-y-4 p-6">
      {/* 标题栏 */}
      <header className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-bold tracking-tight">FileSync · 文件夹内容比对</h1>
          <p className="text-xs text-muted-foreground">基于 SHA256 的备份文件夹对比 · 文件名无关 · 多线程哈希</p>
        </div>
        <ThemeToggle />
      </header>

      {/* 输入区 */}
      <section className="space-y-3 rounded-xl border bg-card p-4 shadow-sm">
        <FolderPicker label="文件夹A" value={pathA} onChange={setPathA} />
        <FolderPicker label="文件夹B" value={pathB} onChange={setPathB} />
        <div className="flex flex-wrap items-center gap-2 pt-1">
          <Button onClick={handleStart} disabled={running || !pathA || !pathB}>
            {running ? "比对中…" : "开始比对"}
          </Button>
          <Button variant="destructive" onClick={handleStop} disabled={!running}>
            停止扫描
          </Button>
          <Button variant="outline" onClick={handleExport} disabled={running || !result}>
            导出结果 CSV
          </Button>
          <span className="ml-auto text-xs text-muted-foreground">
            大小不同的文件自动跳过哈希，双击结果行可打开所在文件夹
          </span>
        </div>
      </section>

      {/* 消息条 */}
      {message && (
        <div
          className={`rounded-lg border px-3 py-2 text-sm ${
            messageType === "error"
              ? "border-destructive/40 bg-destructive/10 text-destructive"
              : "border-border bg-muted text-foreground"
          }`}
        >
          {message}
        </div>
      )}

      {/* 进度区 */}
      <ProgressPanel progress={progress} running={running} onStop={handleStop} />

      {/* 取消提示 + 统计 */}
      {result && (
        <section className="space-y-3">
          {result.stats.cancelled && (
            <div className="rounded-lg border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-sm text-amber-600 dark:text-amber-400">
              扫描已被用户中断，以下为已完成部分的结果。
            </div>
          )}
          <div className="flex flex-wrap items-center gap-x-4 gap-y-1 rounded-xl border bg-card px-4 py-2 text-xs text-muted-foreground shadow-sm">
            <Stat label="A 文件" value={s!.totalA} />
            <Stat label="B 文件" value={s!.totalB} />
            <Stat label="计算哈希" value={s!.hashed} />
            <Stat label="大小跳过" value={s!.skippedBySize} />
            <Stat label="读取失败" value={s!.errorCount} />
            {s!.cancelledCount > 0 && <Stat label="未处理(取消)" value={s!.cancelledCount} />}
            <span className="ml-auto">耗时 {formatDuration(s!.durationMs)}</span>
          </div>
          <ResultsView result={result} />
        </section>
      )}

      {/* 空状态 */}
      {!result && !running && (
        <div className="flex h-52 flex-col items-center justify-center gap-2 rounded-xl border border-dashed text-muted-foreground">
          <span className="text-4xl">🗂️</span>
          <p className="text-sm">选择两个文件夹后点击「开始比对」</p>
        </div>
      )}
    </div>
  );
}

function Stat({ label, value }: { label: string; value: number }) {
  return (
    <span>
      {label} <span className="font-medium text-foreground">{value.toLocaleString()}</span>
    </span>
  );
}

/** 浅色 / 深色主题切换 */
function ThemeToggle() {
  const [dark, setDark] = useState(() => document.documentElement.classList.contains("dark"));
  useEffect(() => {
    document.documentElement.classList.toggle("dark", dark);
    localStorage.setItem("theme", dark ? "dark" : "light");
  }, [dark]);
  return (
    <Button variant="ghost" size="sm" onClick={() => setDark(!dark)} title="切换浅色 / 深色主题">
      {dark ? "🌙 深色" : "☀️ 浅色"}
    </Button>
  );
}
