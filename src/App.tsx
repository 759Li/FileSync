import { useEffect, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { save } from "@tauri-apps/plugin-dialog";
import { Button } from "./components/ui/button";
import { FolderPicker } from "./components/FolderPicker";
import { ProgressPanel } from "./components/ProgressPanel";
import { ResultsView } from "./components/ResultsView";
import { SyncPanel } from "./components/SyncPanel";
import type { CompareResult, ProgressPayload, SyncAction, SyncProgressPayload, SyncResult } from "./types";
import { formatDuration } from "./lib/format";

export default function App(): ReactNode {
  const [pathA, setPathA] = useState("");
  const [pathB, setPathB] = useState("");
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState<ProgressPayload | null>(null);
  const [result, setResult] = useState<CompareResult | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [messageType, setMessageType] = useState<"info" | "error">("info");
  const [syncing, setSyncing] = useState(false);
  const [syncProgress, setSyncProgress] = useState<SyncProgressPayload | null>(null);
  const [syncResult, setSyncResult] = useState<SyncResult | null>(null);
  const [syncPanelOpen, setSyncPanelOpen] = useState(false);

  /* 订阅后端事件：进度 + 最终结果 + 同步进度/结果 */
  useEffect(() => {
    const u1 = listen<ProgressPayload>("compare-progress", (e) => setProgress(e.payload));
    const u2 = listen<CompareResult>("compare-result", (e) => {
      setResult(e.payload);
      setRunning(false);
      setProgress(null);
      setSyncResult(null);
      setSyncProgress(null);
      // 比对完整完成且存在单侧独有文件时，展示同步操作面板
      setSyncPanelOpen(!e.payload.stats.cancelled && e.payload.onlyA.length + e.payload.onlyB.length > 0);
    });
    const u3 = listen<SyncProgressPayload>("sync-progress", (e) => setSyncProgress(e.payload));
    const u4 = listen<SyncResult>("sync-result", (e) => {
      setSyncResult(e.payload);
      setSyncing(false);
      setSyncProgress(null);
    });
    return () => {
      u1.then((f: UnlistenFn) => f());
      u2.then((f: UnlistenFn) => f());
      u3.then((f: UnlistenFn) => f());
      u4.then((f: UnlistenFn) => f());
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
    setSyncResult(null);
    setSyncPanelOpen(false);
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

  const handleSyncAction = async (action: SyncAction) => {
    setMessage(null);
    try {
      await invoke("apply_sync_action", { action });
      setSyncing(true);
      setSyncResult(null);
      setSyncProgress(null);
    } catch (e) {
      notify(String(e), "error");
    }
  };

  const handleIgnoreSync = () => {
    setSyncPanelOpen(false);
    notify("已选择不做处理，可点击统计栏中的「同步操作」重新打开");
  };

  const handleStopSync = () => {
    invoke("stop_sync").catch((e) => notify(String(e), "error"));
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
  const hasUnique = !!result && result.onlyA.length + result.onlyB.length > 0;

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
          <Button onClick={handleStart} disabled={running || syncing || !pathA || !pathB}>
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

      {/* 取消提示 + 统计 + 同步操作 */}
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
            <span className="ml-auto flex items-center gap-3">
              {!syncPanelOpen && hasUnique && !s!.cancelled && (
                <Button variant="outline" size="sm" className="h-6 px-2 text-xs" onClick={() => setSyncPanelOpen(true)}>
                  同步操作
                </Button>
              )}
              耗时 {formatDuration(s!.durationMs)}
            </span>
          </div>
          {syncPanelOpen && !s!.cancelled && (
            <SyncPanel
              onlyACount={result.onlyA.length}
              onlyBCount={result.onlyB.length}
              syncing={syncing}
              syncProgress={syncProgress}
              syncResult={syncResult}
              onAction={handleSyncAction}
              onIgnore={handleIgnoreSync}
              onStop={handleStopSync}
            />
          )}
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
