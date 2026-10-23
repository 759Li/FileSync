import { useEffect, useMemo, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { confirm, save } from "@tauri-apps/plugin-dialog";
import { Button } from "./ui/button";
import { FolderPicker } from "./FolderPicker";
import { Progress } from "./ui/progress";
import { ProgressPanel, openInExplorer } from "./ProgressPanel";
import type {
  DeleteBatchSummary,
  DupDeleteProgressPayload,
  DupDeleteResult,
  DupFileRef,
  DupGroup,
  DupScanProgressPayload,
  DupScanResult,
  DupPurgeResult,
  DupUndoResult,
} from "../types";
import { copyText, cn } from "../lib/utils";
import { formatBytes, formatDuration } from "../lib/format";

type DupTabKey = "dup" | "err";

/** 单文件夹重复文件扫描：枚举 → 大小预筛 → SHA256 内容确认 → 按组展示清单 */
export function DupScanView(): ReactNode {
  const [path, setPath] = useState("");
  const [scanning, setScanning] = useState(false);
  const [progress, setProgress] = useState<DupScanProgressPayload | null>(null);
  const [result, setResult] = useState<DupScanResult | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [messageType, setMessageType] = useState<"info" | "error">("info");
  /** 每组内被选中（建议清理）的文件路径集合，key 为组号 */
  const [selected, setSelected] = useState<Record<number, Set<string>>>({});
  const [tab, setTab] = useState<DupTabKey>("dup");
  /* 回收站：删除中 / 撤销中 / 清空中 三态互斥；进度与批次列表 */
  const [deleting, setDeleting] = useState(false);
  const [restoring, setRestoring] = useState(false);
  const [purging, setPurging] = useState(false);
  const [trashProgress, setTrashProgress] = useState<DupDeleteProgressPayload | null>(null);
  const [batches, setBatches] = useState<DeleteBatchSummary[]>([]);
  /** 最近一次删除产生的批次号（删除完成后展示「撤销本次删除」按钮） */
  const [lastBatchId, setLastBatchId] = useState<string | null>(null);

  const trashBusy = deleting || restoring || purging;

  /* 订阅后端事件：扫描进度 + 扫描结果 + 回收站操作进度/结果 + 回收站状态 */
  useEffect(() => {
    const u1 = listen<DupScanProgressPayload>("dup-scan-progress", (e) => setProgress(e.payload));
    const u2 = listen<DupScanResult>("dup-scan-result", (e) => {
      setResult(e.payload);
      setScanning(false);
      setProgress(null);
      // 默认选中每组除首个文件外的副本（首个按路径排序，作为建议保留项）
      const sel: Record<number, Set<string>> = {};
      for (const g of e.payload.groups) {
        sel[g.id] = new Set(g.files.slice(1).map((f) => f.path));
      }
      setSelected(sel);
      setTab("dup");
    });
    const u3 = listen<DupDeleteProgressPayload>("dup-delete-progress", (e) =>
      setTrashProgress(e.payload)
    );
    const u4 = listen<DupDeleteResult>("dup-delete-result", (e) => {
      const r = e.payload;
      setDeleting(false);
      setTrashProgress(null);
      if (r.batchId) setLastBatchId(r.batchId);
      else setLastBatchId(null);
      const parts = [`已移入回收站 ${r.deleted} 个文件`];
      if (r.freedBytes > 0) parts.push(`释放 ${formatBytes(r.freedBytes)}`);
      if (r.skipped > 0) parts.push(`跳过 ${r.skipped} 个`);
      if (r.failed > 0) parts.push(`失败 ${r.failed} 个`);
      if (r.cancelled) parts.push("（已停止）");
      notify(parts.join("，"), r.failed > 0 ? "error" : "info");
    });
    const u5 = listen<DupUndoResult>("dup-undo-result", (e) => {
      const r = e.payload;
      setRestoring(false);
      setTrashProgress(null);
      setLastBatchId(null);
      const parts = [`已恢复 ${r.restored} 个文件到原位置`];
      if (r.conflicts > 0) parts.push(`冲突跳过 ${r.conflicts} 个`);
      if (r.failed > 0) parts.push(`失败 ${r.failed} 个`);
      if (r.cancelled) parts.push("（已停止）");
      notify(parts.join("，"), r.failed > 0 ? "error" : "info");
    });
    const u6 = listen<DupPurgeResult>("dup-purge-result", (e) => {
      const r = e.payload;
      setPurging(false);
      setTrashProgress(null);
      notify(
        `已彻底删除 ${r.purgedFiles} 个文件（${formatBytes(r.purgedBytes)}）` +
          (r.failed > 0 ? `，失败 ${r.failed} 个` : ""),
        r.failed > 0 ? "error" : "info"
      );
    });
    const u7 = listen<DeleteBatchSummary[]>("dup-trash-state", (e) => setBatches(e.payload));
    return () => {
      u1.then((f: UnlistenFn) => f());
      u2.then((f: UnlistenFn) => f());
      u3.then((f: UnlistenFn) => f());
      u4.then((f: UnlistenFn) => f());
      u5.then((f: UnlistenFn) => f());
      u6.then((f: UnlistenFn) => f());
      u7.then((f: UnlistenFn) => f());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  /* 挂载时加载回收站批次（批次记录持久化，重启后仍可撤销） */
  useEffect(() => {
    invoke<DeleteBatchSummary[]>("list_delete_batches")
      .then(setBatches)
      .catch(() => {});
  }, []);

  const notify = (text: string, type: "info" | "error" = "info") => {
    setMessage(text);
    setMessageType(type);
  };

  const handleStart = async () => {
    setMessage(null);
    setResult(null);
    setProgress(null);
    setSelected({});
    setLastBatchId(null);
    try {
      await invoke("start_dup_scan", { path });
      setScanning(true);
    } catch (e) {
      notify(String(e), "error");
    }
  };

  const handleStop = () => {
    invoke("stop_dup_scan").catch((e) => notify(String(e), "error"));
  };

  const handleExport = async () => {
    const p = await save({
      title: "导出重复文件清单 CSV",
      defaultPath: "重复文件清单.csv",
      filters: [{ name: "CSV 文件", extensions: ["csv"] }],
    });
    if (!p) return;
    try {
      await invoke("export_dup_csv", { savePath: p });
      notify(`已导出到：${p}`);
    } catch (e) {
      notify(String(e), "error");
    }
  };

  const toggleFile = (groupId: number, filePath: string) => {
    setSelected((prev) => {
      const next = { ...prev };
      const set = new Set(next[groupId] ?? []);
      if (set.has(filePath)) set.delete(filePath);
      else set.add(filePath);
      next[groupId] = set;
      return next;
    });
  };

  const setGroupSelection = (groupId: number, paths: string[]) => {
    setSelected((prev) => ({ ...prev, [groupId]: new Set(paths) }));
  };

  /* 复制全部选中（建议清理）的文件路径，便于粘贴进清理脚本或资源管理器 */
  const handleCopySelected = async () => {
    if (selCount === 0) {
      notify("当前没有选中的文件", "error");
      return;
    }
    const ok = await copyText(selPaths.join("\n"));
    notify(
      ok ? `已复制 ${selCount} 个文件路径（选中副本共 ${formatBytes(selBytes)}）` : "复制失败，请手动选择",
      ok ? "info" : "error"
    );
  };

  /* 选中副本统计（数量、总占用空间、路径列表、待删除载荷） */
  const { selCount, selBytes, selPaths, selFiles } = useMemo(() => {
    const paths: string[] = [];
    const files: DupFileRef[] = [];
    let bytes = 0;
    if (result) {
      for (const g of result.groups) {
        const sel = selected[g.id];
        if (!sel) continue;
        for (const f of g.files) {
          if (sel.has(f.path)) {
            paths.push(f.path);
            files.push({ path: f.path, size: f.size });
            bytes += f.size;
          }
        }
      }
    }
    return { selCount: paths.length, selBytes: bytes, selPaths: paths, selFiles: files };
  }, [result, selected]);

  /* 全部副本都被选中的组数（删除后该组内容将只存在于回收站，确认时提醒） */
  const fullGroupCount = useMemo(() => {
    if (!result) return 0;
    let n = 0;
    for (const g of result.groups) {
      const sel = selected[g.id];
      if (sel && g.files.length > 0 && g.files.every((f) => sel.has(f.path))) n++;
    }
    return n;
  }, [result, selected]);

  /* 一键删除：确认后移入 App 回收站（可撤销） */
  const handleDelete = async () => {
    if (selCount === 0) {
      notify("当前没有选中的文件", "error");
      return;
    }
    let text =
      `即将把 ${selCount} 个选中的重复文件移入 FileSync 回收站（共 ${formatBytes(selBytes)}）。\n\n` +
      `文件不会进入系统回收站，但可在下方「回收站」中一键恢复到原位置。确认删除？`;
    if (fullGroupCount > 0) {
      text += `\n\n⚠ 注意：其中 ${fullGroupCount} 个组的全部副本都被选中，删除后这些内容将只存在于回收站中。`;
    }
    const ok = await confirm(text, {
      title: "删除重复文件",
      kind: "warning",
      okLabel: "移入回收站",
      cancelLabel: "取消",
    });
    if (!ok) return;
    setMessage(null);
    setLastBatchId(null);
    try {
      await invoke("delete_dup_files", { files: selFiles });
      setDeleting(true);
    } catch (e) {
      notify(String(e), "error");
    }
  };

  /* 撤销删除：把回收站批次中的文件恢复到原位置 */
  const handleUndo = async (batchId: string) => {
    const ok = await confirm(
      "将把该批次的文件恢复到原来的位置；原位置已存在的同名文件会跳过（不覆盖）。确认恢复？",
      { title: "撤销删除", kind: "info", okLabel: "恢复文件", cancelLabel: "取消" }
    );
    if (!ok) return;
    setMessage(null);
    try {
      await invoke("undo_delete_dup", { batchId });
      setRestoring(true);
    } catch (e) {
      notify(String(e), "error");
    }
  };

  /* 彻底清空回收站（不可恢复） */
  const handlePurge = async () => {
    const totalFiles = batches.reduce((a, b) => a + b.fileCount, 0);
    const totalBytes = batches.reduce((a, b) => a + b.totalBytes, 0);
    const ok = await confirm(
      `将永久删除回收站中的全部 ${totalFiles} 个文件（共 ${formatBytes(totalBytes)}），此操作不可恢复！确认彻底清空？`,
      { title: "清空回收站", kind: "warning", okLabel: "永久删除", cancelLabel: "取消" }
    );
    if (!ok) return;
    setMessage(null);
    setLastBatchId(null);
    try {
      await invoke("purge_trash");
      setPurging(true);
    } catch (e) {
      notify(String(e), "error");
    }
  };

  const handleOpenTrash = () => {
    invoke("open_trash_dir").catch((e) => notify(String(e), "error"));
  };

  /* 停止回收站任务（删除 / 撤销 / 清空共用取消标志；已处理的生效，未处理的保留） */
  const handleStopTrash = () => {
    invoke("stop_dup_delete").catch((e) => notify(String(e), "error"));
  };

  const s = result?.stats;
  const trashTotalBytes = batches.reduce((a, b) => a + b.totalBytes, 0);
  const trashTotalFiles = batches.reduce((a, b) => a + b.fileCount, 0);

  return (
    <div className="space-y-4">
      {/* 输入区 */}
      <section className="space-y-3 rounded-xl border bg-card p-4 shadow-sm">
        <FolderPicker label="目标文件夹" value={path} onChange={setPath} />
        <div className="flex flex-wrap items-center gap-2 pt-1">
          <Button onClick={handleStart} disabled={scanning || !path || trashBusy}>
            {scanning ? "扫描中…" : "开始扫描"}
          </Button>
          <Button variant="destructive" onClick={handleStop} disabled={!scanning}>
            停止扫描
          </Button>
          <Button variant="outline" onClick={handleExport} disabled={scanning || !result || trashBusy}>
            导出清单 CSV
          </Button>
          <Button variant="outline" onClick={handleCopySelected} disabled={!result || selCount === 0 || trashBusy}>
            复制选中路径（{selCount} 个 · {formatBytes(selBytes)}）
          </Button>
          <Button
            variant="destructive"
            onClick={handleDelete}
            disabled={!result || selCount === 0 || trashBusy}
            title="把选中的重复副本移入 FileSync 回收站（可一键撤销恢复）"
          >
            🗑 删除选中副本（{selCount} 个 · {formatBytes(selBytes)}）
          </Button>
          <span className="ml-auto text-xs text-muted-foreground">
            先按大小分组预筛，再以 SHA256 内容确认；双击行打开所在文件夹
          </span>
        </div>
      </section>

      {/* 消息条 */}
      {message && (
        <div className="flex flex-wrap items-center gap-3">
          <div
            className={cn(
              "flex-1 rounded-lg border px-3 py-2 text-sm",
              messageType === "error"
                ? "border-destructive/40 bg-destructive/10 text-destructive"
                : "border-border bg-muted text-foreground"
            )}
          >
            {message}
          </div>
          {lastBatchId && !trashBusy && (
            <Button variant="outline" size="sm" onClick={() => handleUndo(lastBatchId)}>
              ↩ 撤销本次删除
            </Button>
          )}
        </div>
      )}

      {/* 回收站操作进度（删除 / 撤销 / 清空共用，可停止） */}
      {trashProgress && (
        <section className="space-y-3 rounded-xl border bg-card p-4 shadow-sm">
          <div className="flex items-center justify-between gap-4">
            <span className="text-sm font-medium">{trashPhaseText(trashProgress.phase)}</span>
            <Button variant="destructive" size="sm" onClick={handleStopTrash}>
              停止
            </Button>
          </div>
          <Progress value={trashProgress.percent} />
          <div className="flex items-center justify-between text-xs text-muted-foreground">
            <span>
              已处理 {trashProgress.processed} / {trashProgress.total} 个文件（
              {trashProgress.percent.toFixed(1)}%）
            </span>
            <span>已处理的生效，未处理的保留</span>
          </div>
          {trashProgress.currentFile && (
            <p
              className="truncate rounded-md bg-muted px-2 py-1.5 font-mono text-xs text-muted-foreground"
              title={trashProgress.currentFile}
            >
              {trashProgress.currentFile}
            </p>
          )}
        </section>
      )}

      {/* 回收站：持久化的删除批次，可逐批恢复 / 彻底清空 */}
      {batches.length > 0 && (
        <section className="space-y-2 rounded-xl border bg-card p-4 shadow-sm">
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-sm font-medium">🗑 回收站</span>
            <span className="text-xs text-muted-foreground">
              共 {batches.length} 个删除批次 · {trashTotalFiles} 个文件 · {formatBytes(trashTotalBytes)}
            </span>
            <div className="ml-auto flex items-center gap-2">
              <Button variant="outline" size="sm" onClick={handleOpenTrash} disabled={trashBusy}>
                打开回收站文件夹
              </Button>
              <Button variant="destructive" size="sm" onClick={handlePurge} disabled={trashBusy}>
                清空回收站
              </Button>
            </div>
          </div>
          <div className="divide-y rounded-lg border">
            {batches.map((b) => (
              <div key={b.id} className="flex flex-wrap items-center gap-x-3 gap-y-1 px-3 py-2 text-xs">
                <span className="font-mono text-muted-foreground">{formatBatchTime(b.createdAt)}</span>
                <span>
                  {b.fileCount} 个文件 · {formatBytes(b.totalBytes)}
                </span>
                <span className="truncate text-muted-foreground" title={b.root}>
                  来自 {b.root}
                </span>
                <Button
                  variant="outline"
                  size="sm"
                  className="ml-auto h-6 px-2 text-xs"
                  onClick={() => handleUndo(b.id)}
                  disabled={trashBusy}
                  title="把该批次的文件恢复到原位置"
                >
                  ↩ 恢复
                </Button>
              </div>
            ))}
          </div>
          <p className="text-xs text-muted-foreground">
            删除的重复文件先移入 App 回收站（非系统回收站），可随时恢复到原位置；「清空回收站」后不可恢复。
            跨磁盘移动时可能短暂占用双倍空间。
          </p>
        </section>
      )}

      {/* 扫描进度区 */}
      <ProgressPanel progress={progress} running={scanning} onStop={handleStop} title="目标文件夹" />

      {/* 取消提示 + 统计 + 结果 */}
      {result && (
        <section className="space-y-3">
          {result.stats.cancelled && (
            <div className="rounded-lg border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-sm text-amber-600 dark:text-amber-400">
              扫描已被用户中断，以下为已完成部分的结果。
            </div>
          )}
          <div className="flex flex-wrap items-center gap-x-4 gap-y-1 rounded-xl border bg-card px-4 py-2 text-xs text-muted-foreground shadow-sm">
            <Stat label="文件总数" value={s!.totalFiles} />
            <Stat label="计算哈希" value={s!.hashed} />
            <Stat label="大小跳过" value={s!.skippedBySize} />
            <Stat label="重复组" value={s!.dupGroupCount} />
            <Stat label="重复文件" value={s!.dupFileCount} />
            <Stat label="可释放空间" value={formatBytes(s!.reclaimableBytes)} />
            <Stat label="读取失败" value={s!.errorCount} />
            {s!.cancelledCount > 0 && <Stat label="未处理(取消)" value={s!.cancelledCount} />}
            <span className="ml-auto">耗时 {formatDuration(s!.durationMs)}</span>
          </div>

          {/* Tab 栏 */}
          <div className="flex gap-1 rounded-lg bg-muted p-1">
            <button onClick={() => setTab("dup")} className={tabClass(tab === "dup")}>
              重复文件
              <span className={countClass(s!.dupGroupCount)}>{s!.dupGroupCount}组</span>
            </button>
            <button onClick={() => setTab("err")} className={tabClass(tab === "err")}>
              读取失败
              <span className={countClass(s!.errorCount)}>{s!.errorCount}条</span>
            </button>
          </div>

          {/* 内容区 */}
          <div className="max-h-[calc(100vh-430px)] min-h-[200px] overflow-y-auto rounded-xl border bg-card shadow-sm">
            {tab === "dup" ? (
              result.groups.length === 0 ? (
                <Empty text="未发现内容重复的文件" />
              ) : (
                <div className="divide-y">
                  {result.groups.map((g) => (
                    <DupGroupCard
                      key={g.id}
                      group={g}
                      selected={selected[g.id] ?? new Set<string>()}
                      onToggle={toggleFile}
                      onSelectAll={(paths) => setGroupSelection(g.id, paths)}
                      onSelectNone={() => setGroupSelection(g.id, [])}
                    />
                  ))}
                </div>
              )
            ) : result.errors.length === 0 ? (
              <Empty text="没有读取失败的文件" />
            ) : (
              <div className="divide-y">
                {result.errors.map((e, i) => (
                  <div key={i} className="px-3 py-2">
                    <p className="truncate font-mono text-xs" title={e.path}>
                      {e.path}
                    </p>
                    <p className="text-xs text-destructive">{e.error}</p>
                  </div>
                ))}
              </div>
            )}
          </div>
        </section>
      )}

      {/* 空状态 */}
      {!result && !scanning && (
        <div className="flex h-52 flex-col items-center justify-center gap-2 rounded-xl border border-dashed text-muted-foreground">
          <span className="text-4xl">🔍</span>
          <p className="text-sm">选择文件夹后点击「开始扫描」，查找文件夹内部互相重复的文件</p>
        </div>
      )}
    </div>
  );
}

/* ---------------- 工具函数 ---------------- */

/** 回收站操作阶段的中文说明 */
function trashPhaseText(phase: string): string {
  switch (phase) {
    case "deleting":
      return "正在移入回收站";
    case "restoring":
      return "正在恢复到原位置";
    case "purging":
      return "正在彻底清空回收站（不可恢复）";
    default:
      return "回收站操作进行中";
  }
}

/** 批次创建时间格式化（Unix 毫秒 → 本地时间） */
function formatBatchTime(ms: number): string {
  return new Date(ms).toLocaleString("zh-CN", { hour12: false });
}

/* ---------------- 统计项 ---------------- */

function Stat({ label, value }: { label: string; value: number | string }) {
  return (
    <span>
      {label}{" "}
      <span className="font-medium text-foreground">
        {typeof value === "number" ? value.toLocaleString() : value}
      </span>
    </span>
  );
}

/* ---------------- Tab 样式 ---------------- */

function tabClass(active: boolean): string {
  return cn(
    "flex flex-1 items-center justify-center gap-1.5 rounded-md px-3 py-1.5 text-sm font-medium transition-colors",
    active ? "bg-background shadow text-foreground" : "text-muted-foreground hover:text-foreground"
  );
}

function countClass(count: number): string {
  return cn(
    "rounded-full px-1.5 text-xs",
    count > 0 ? "bg-primary text-primary-foreground" : "bg-secondary text-muted-foreground"
  );
}

/* ---------------- 复制按钮 ---------------- */

function CopyBtn({ text, label }: { text: string; label: string }) {
  return (
    <button
      onClick={(e) => {
        e.stopPropagation();
        copyText(text);
      }}
      className="shrink-0 rounded px-1.5 py-0.5 text-xs text-muted-foreground opacity-0 transition-opacity hover:bg-accent hover:text-foreground group-hover/row:opacity-100"
      title={`复制${label}`}
    >
      复制
    </button>
  );
}

/* ---------------- 重复组卡片 ---------------- */

interface DupGroupCardProps {
  group: DupGroup;
  /** 组内被选中（建议清理）的文件路径 */
  selected: Set<string>;
  onToggle: (groupId: number, filePath: string) => void;
  onSelectAll: (paths: string[]) => void;
  onSelectNone: () => void;
}

function DupGroupCard({ group, selected, onToggle, onSelectAll, onSelectNone }: DupGroupCardProps) {
  return (
    <div className="p-3">
      {/* 组头 */}
      <div className="mb-2 flex flex-wrap items-center gap-2 text-xs">
        <span className="rounded bg-primary px-1.5 py-0.5 font-medium text-primary-foreground">第 {group.id} 组</span>
        <span className="text-muted-foreground">
          {formatBytes(group.size)} × {group.files.length} 个
        </span>
        <span className="font-medium text-amber-600 dark:text-amber-400" title="组内仅保留 1 个副本时可释放的空间">
          可释放 {formatBytes(group.reclaimable)}
        </span>
        <code className="rounded bg-muted px-1.5 py-0.5 font-mono" title={group.hash}>
          {group.hash.slice(0, 16)}…
        </code>
        <CopyBtn text={group.hash} label="完整哈希" />
        <div className="ml-auto flex items-center gap-1">
          <Button
            variant="ghost"
            size="sm"
            className="h-6 px-2 text-xs"
            onClick={() => onSelectAll(group.files.map((f) => f.path))}
          >
            全选
          </Button>
          <Button variant="ghost" size="sm" className="h-6 px-2 text-xs" onClick={onSelectNone}>
            全不选
          </Button>
        </div>
      </div>
      {/* 组内文件 */}
      <div className="space-y-0.5">
        {group.files.map((f, i) => {
          const isKeeper = i === 0; // 按路径排序的首个文件，默认作为保留副本
          return (
            <div
              key={f.path}
              className="group/row flex items-center gap-2 rounded-md px-2 py-1 hover:bg-accent"
              title="双击打开所在文件夹"
            >
              <input
                type="checkbox"
                checked={selected.has(f.path)}
                onChange={() => onToggle(group.id, f.path)}
                className="h-3.5 w-3.5 shrink-0 accent-primary"
                title={selected.has(f.path) ? "已选中（建议清理）" : "未选中（保留）"}
              />
              {isKeeper && (
                <span
                  className="shrink-0 rounded bg-emerald-500/15 px-1.5 py-0.5 text-xs font-medium text-emerald-600 dark:text-emerald-400"
                  title="按路径排序的首个文件，默认作为保留副本"
                >
                  保留
                </span>
              )}
              <span className="truncate font-mono text-xs" title={f.path} onDoubleClick={() => openInExplorer(f.path)}>
                {f.path}
              </span>
              {!isKeeper && (
                <button
                  onClick={() => onSelectAll(group.files.filter((x) => x.path !== f.path).map((x) => x.path))}
                  className="shrink-0 rounded px-1.5 py-0.5 text-xs text-muted-foreground opacity-0 transition-opacity hover:bg-accent hover:text-foreground group-hover/row:opacity-100"
                  title="仅保留此文件：选中组内其他全部副本"
                >
                  仅保留
                </button>
              )}
              <span className="ml-auto shrink-0 text-xs text-muted-foreground">{formatBytes(f.size)}</span>
              <CopyBtn text={f.path} label="文件路径" />
            </div>
          );
        })}
      </div>
    </div>
  );
}

function Empty({ text }: { text: string }) {
  return <div className="flex h-40 items-center justify-center text-sm text-muted-foreground">{text}</div>;
}
