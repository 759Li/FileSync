import { useEffect, useRef, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { save } from "@tauri-apps/plugin-dialog";
import { Button } from "./components/ui/button";
import { FolderPicker } from "./components/FolderPicker";
import { ProgressPanel } from "./components/ProgressPanel";
import { ResultsView } from "./components/ResultsView";
import { SyncPanel } from "./components/SyncPanel";
import { DupScanView } from "./components/DupScanView";
import type {
  CompareResult,
  ProgressPayload,
  SyncAction,
  SyncProgressPayload,
  SyncResult,
  UndoProgressPayload,
  UndoResult,
} from "./types";
import { formatDuration } from "./lib/format";
import { cn } from "./lib/utils";

/** 顶部功能模式：文件夹比对 / 重复文件扫描 */
type Mode = "compare" | "dup";

export default function App(): ReactNode {
  const [mode, setMode] = useState<Mode>("compare");
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
  const [undoing, setUndoing] = useState(false);
  const [undoProgress, setUndoProgress] = useState<UndoProgressPayload | null>(null);
  const [undoResult, setUndoResult] = useState<UndoResult | null>(null);
  /** 当前累计可撤销的复制文件数（来自最近 sync-result / undo-result） */
  const [undoable, setUndoable] = useState(0);
  /** 结果列表中勾选待一致化的文件路径（仅 A / 仅 B 各自独立） */
  const [selectedA, setSelectedA] = useState<Set<string>>(new Set());
  const [selectedB, setSelectedB] = useState<Set<string>>(new Set());
  /** 同步 / 撤销成功后是否正在自动刷新结果列表 */
  const [refreshing, setRefreshing] = useState(false);
  /** 标记当前 compare-result 是否为自动刷新（区分手动比对，保留同步摘要与撤销记录） */
  const autoRefreshRef = useRef(false);
  /** 重复文件扫描是否正在运行（仅用于任务期间禁用模式切换） */
  const [dupRunning, setDupRunning] = useState(false);

  /* 订阅后端事件：进度 + 最终结果 + 同步/撤销进度与结果 */
  useEffect(() => {
    const u1 = listen<ProgressPayload>("compare-progress", (e) => setProgress(e.payload));
    const u2 = listen<CompareResult>("compare-result", (e) => {
      const isAutoRefresh = autoRefreshRef.current;
      autoRefreshRef.current = false;
      setRunning(false);
      setProgress(null);
      if (isAutoRefresh) {
        setRefreshing(false);
        if (e.payload.stats.cancelled) {
          // 刷新被中断：保留刷新前的结果与同步面板，撤销入口仍然可用
          notify("结果列表刷新被中断，已保留刷新前的结果");
          return;
        }
        // 自动刷新：保留同步 / 撤销摘要与撤销记录，只替换结果列表
        setResult(e.payload);
        setSelectedA(new Set());
        setSelectedB(new Set());
        setSyncPanelOpen(true);
        notify("结果列表已按最新状态刷新");
        return;
      }
      setResult(e.payload);
      // 新结果对应新的文件清单，清空上一轮的勾选
      setSelectedA(new Set());
      setSelectedB(new Set());
      setSyncResult(null);
      setSyncProgress(null);
      setUndoResult(null);
      setUndoProgress(null);
      setUndoable(0);
      setRefreshing(false);
      // 比对完整完成且存在单侧独有文件时，展示同步操作面板
      setSyncPanelOpen(!e.payload.stats.cancelled && e.payload.onlyA.length + e.payload.onlyB.length > 0);
    });
    const u3 = listen<SyncProgressPayload>("sync-progress", (e) => setSyncProgress(e.payload));
    const u4 = listen<SyncResult>("sync-result", (e) => {
      setSyncResult(e.payload);
      setSyncing(false);
      setSyncProgress(null);
      setUndoable(e.payload.undoable);
      // 同步后结果列表仍是旧快照，清空勾选避免重复操作
      setSelectedA(new Set());
      setSelectedB(new Set());
      // 有文件成功复制时，自动重新比对并刷新结果列表（保留撤销记录）
      if (e.payload.copied > 0) startAutoRefresh();
    });
    const u5 = listen<UndoProgressPayload>("undo-progress", (e) => setUndoProgress(e.payload));
    const u6 = listen<UndoResult>("undo-result", (e) => {
      setUndoResult(e.payload);
      setUndoing(false);
      setUndoProgress(null);
      setUndoable(e.payload.remaining);
      // 有文件被撤销删除时，自动重新比对并刷新结果列表
      if (e.payload.removed > 0) startAutoRefresh();
    });
    // 重复扫描事件：仅跟踪运行标志，用于任务期间禁用模式切换
    const u7 = listen("dup-scan-progress", () => setDupRunning(true));
    const u8 = listen("dup-scan-result", () => setDupRunning(false));
    return () => {
      u1.then((f: UnlistenFn) => f());
      u2.then((f: UnlistenFn) => f());
      u3.then((f: UnlistenFn) => f());
      u4.then((f: UnlistenFn) => f());
      u5.then((f: UnlistenFn) => f());
      u6.then((f: UnlistenFn) => f());
      u7.then((f: UnlistenFn) => f());
      u8.then((f: UnlistenFn) => f());
    };
  }, []);

  const notify = (text: string, type: "info" | "error" = "info") => {
    setMessage(text);
    setMessageType(type);
  };

  /** 重新比对以刷新结果列表（同步 / 撤销成功后自动触发；后端保留撤销记录） */
  const startAutoRefresh = () => {
    autoRefreshRef.current = true;
    setRunning(true);
    setProgress(null);
    setRefreshing(true);
    invoke("refresh_compare").catch((e) => {
      autoRefreshRef.current = false;
      setRunning(false);
      setRefreshing(false);
      notify(`结果列表自动刷新失败：${e}`, "error");
    });
  };

  const handleStart = async () => {
    setMessage(null);
    setResult(null);
    setProgress(null);
    setSyncResult(null);
    setSyncPanelOpen(false);
    setUndoResult(null);
    setUndoable(0);
    setSelectedA(new Set());
    setSelectedB(new Set());
    // 先置为运行中，避免任务在 invoke 返回前完成时错过 / 覆盖状态
    setRunning(true);
    try {
      await invoke("start_compare", { pathA, pathB });
    } catch (e) {
      setRunning(false);
      notify(String(e), "error");
    }
  };

  const handleStop = () => {
    invoke("stop_compare").catch((e) => notify(String(e), "error"));
  };

  const handleSyncAction = async (action: SyncAction) => {
    setMessage(null);
    setSyncing(true);
    setSyncResult(null);
    setSyncProgress(null);
    setUndoResult(null);
    try {
      await invoke("apply_sync_action", { action });
    } catch (e) {
      setSyncing(false);
      notify(String(e), "error");
    }
  };

  /** 勾选文件即表示要同步，确保操作面板可见（用户可能已点过「不做处理」） */
  const ensureSyncPanelOpen = () => {
    if (result && !result.stats.cancelled) setSyncPanelOpen(true);
  };

  /** 勾选 / 取消勾选结果列表中的单个独有文件 */
  const toggleSelected = (side: "A" | "B", path: string) => {
    const willSelect = !(side === "A" ? selectedA : selectedB).has(path);
    const setter = side === "A" ? setSelectedA : setSelectedB;
    setter((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
    if (willSelect) ensureSyncPanelOpen();
  };

  /** 全选 / 全不选某一侧的独有文件 */
  const selectAllSelected = (side: "A" | "B", checked: boolean) => {
    const files = side === "A" ? result?.onlyA ?? [] : result?.onlyB ?? [];
    const setter = side === "A" ? setSelectedA : setSelectedB;
    setter(checked ? new Set(files.map((f) => f.path)) : new Set());
    if (checked && files.length > 0) ensureSyncPanelOpen();
  };

  /** 仅把勾选的文件复制到另一侧（A 勾选 → B，B 勾选 → A） */
  const handleSyncSelected = async () => {
    if (selectedA.size === 0 && selectedB.size === 0) return;
    setMessage(null);
    setSyncing(true);
    setSyncResult(null);
    setSyncProgress(null);
    setUndoResult(null);
    try {
      await invoke("apply_sync_selected", {
        selectedA: Array.from(selectedA),
        selectedB: Array.from(selectedB),
      });
    } catch (e) {
      setSyncing(false);
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

  const handleUndo = async () => {
    setMessage(null);
    setUndoing(true);
    setUndoResult(null);
    setUndoProgress(null);
    try {
      await invoke("undo_sync");
    } catch (e) {
      setUndoing(false);
      notify(String(e), "error");
    }
  };

  const handleStopUndo = () => {
    invoke("stop_undo").catch((e) => notify(String(e), "error"));
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
          <h1 className="text-xl font-bold tracking-tight">
            FileSync · {mode === "compare" ? "文件夹内容比对" : "重复文件扫描"}
          </h1>
          <p className="text-xs text-muted-foreground">
            {mode === "compare"
              ? "基于 SHA256 的备份文件夹对比 · 文件名无关 · 多线程哈希"
              : "单文件夹重复文件查找 · 大小预筛 + SHA256 内容确认 · 多线程哈希"}
          </p>
        </div>
        <ThemeToggle />
      </header>

      {/* 功能模式切换 */}
      <div className="flex gap-1 rounded-lg bg-muted p-1">
        <button
          onClick={() => setMode("compare")}
          disabled={running || dupRunning}
          className={cn(
            "flex flex-1 items-center justify-center gap-1.5 rounded-md px-3 py-1.5 text-sm font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-50",
            mode === "compare" ? "bg-background shadow text-foreground" : "text-muted-foreground hover:text-foreground"
          )}
        >
          📁 文件夹比对
        </button>
        <button
          onClick={() => setMode("dup")}
          disabled={running || dupRunning}
          className={cn(
            "flex flex-1 items-center justify-center gap-1.5 rounded-md px-3 py-1.5 text-sm font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-50",
            mode === "dup" ? "bg-background shadow text-foreground" : "text-muted-foreground hover:text-foreground"
          )}
        >
          🔍 重复文件扫描
        </button>
      </div>

      {/* 重复扫描视图：始终保持挂载（隐藏而非卸载），
          避免删除 / 撤销 / 清空等任务进行中切换模式时丢失事件与扫描结果 */}
      <div className={mode === "dup" ? undefined : "hidden"}>
        <DupScanView />
      </div>

      {mode !== "dup" && (
        <>
          {/* 输入区 */}
          <section className="space-y-3 rounded-xl border bg-card p-4 shadow-sm">
            <FolderPicker label="文件夹A" value={pathA} onChange={setPathA} />
            <FolderPicker label="文件夹B" value={pathB} onChange={setPathB} />
            <div className="flex flex-wrap items-center gap-2 pt-1">
              <Button onClick={handleStart} disabled={running || syncing || undoing || !pathA || !pathB}>
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
          <ProgressPanel
            progress={progress}
            running={running}
            onStop={handleStop}
            title={
              refreshing
                ? progress?.side === "B"
                  ? "刷新列表 · 文件夹 B"
                  : "刷新列表 · 文件夹 A"
                : progress?.side === "B"
                  ? "文件夹 B"
                  : "文件夹 A"
            }
          />

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
                  {!syncPanelOpen && !s!.cancelled && (hasUnique || undoable > 0) && (
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
                  selectedACount={selectedA.size}
                  selectedBCount={selectedB.size}
                  syncing={syncing}
                  refreshing={refreshing}
                  syncProgress={syncProgress}
                  syncResult={syncResult}
                  undoable={undoable}
                  undoing={undoing}
                  undoProgress={undoProgress}
                  undoResult={undoResult}
                  onAction={handleSyncAction}
                  onSyncSelected={handleSyncSelected}
                  onIgnore={handleIgnoreSync}
                  onStop={handleStopSync}
                  onUndo={handleUndo}
                  onStopUndo={handleStopUndo}
                />
              )}
              <ResultsView
                result={result}
                selectedA={selectedA}
                selectedB={selectedB}
                onToggleSelected={toggleSelected}
                onSelectAll={selectAllSelected}
              />
            </section>
          )}

          {/* 空状态 */}
          {!result && !running && (
            <div className="flex h-52 flex-col items-center justify-center gap-2 rounded-xl border border-dashed text-muted-foreground">
              <span className="text-4xl">🗂️</span>
              <p className="text-sm">选择两个文件夹后点击「开始比对」</p>
            </div>
          )}
        </>
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
