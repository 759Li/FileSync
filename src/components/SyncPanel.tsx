import { useState } from "react";
import type { SyncAction, SyncProgressPayload, SyncResult, UndoProgressPayload, UndoResult } from "../types";
import { formatDuration } from "../lib/format";
import { Button } from "./ui/button";
import { Progress } from "./ui/progress";
import { cn } from "../lib/utils";

interface SyncPanelProps {
  /** A 独有文件数 */
  onlyACount: number;
  /** B 独有文件数 */
  onlyBCount: number;
  syncing: boolean;
  syncProgress: SyncProgressPayload | null;
  syncResult: SyncResult | null;
  /** 当前累计可撤销的复制文件数 */
  undoable: number;
  undoing: boolean;
  undoProgress: UndoProgressPayload | null;
  undoResult: UndoResult | null;
  onAction: (action: SyncAction) => void;
  onIgnore: () => void;
  onStop: () => void;
  onUndo: () => void;
  onStopUndo: () => void;
}

/** 比对完成后的同步操作面板：一键一致化 / 单向复制 / 不做处理 / 撤销同步 */
export function SyncPanel({
  onlyACount,
  onlyBCount,
  syncing,
  syncProgress,
  syncResult,
  undoable,
  undoing,
  undoProgress,
  undoResult,
  onAction,
  onIgnore,
  onStop,
  onUndo,
  onStopUndo,
}: SyncPanelProps) {
  const [showErrors, setShowErrors] = useState(false);
  const hasUnique = onlyACount + onlyBCount > 0;
  const busy = syncing || undoing;

  return (
    <section className="space-y-3 rounded-xl border bg-card p-4 shadow-sm">
      {/* 标题 */}
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div>
          <h2 className="text-sm font-semibold">后续操作 · 让两侧文件夹保持一致</h2>
          <p className="text-xs text-muted-foreground">
            A 独有 <span className="font-medium text-foreground">{onlyACount.toLocaleString()}</span> 个 · B 独有{" "}
            <span className="font-medium text-foreground">{onlyBCount.toLocaleString()}</span> 个
            {hasUnique && "（同名不同内容的文件会自动跳过，不会覆盖）"}
          </p>
        </div>
      </div>

      {/* 同步进行中：进度条 */}
      {syncing && (
        <div className="space-y-2">
          <div className="flex items-center justify-between gap-4">
            <span className="text-sm font-medium">正在复制文件…</span>
            <Button variant="destructive" size="sm" onClick={onStop}>
              停止同步
            </Button>
          </div>
          <Progress value={syncProgress?.percent ?? 0} />
          <div className="flex items-center justify-between text-xs text-muted-foreground">
            <span>
              已复制 {syncProgress?.copied ?? 0} / {syncProgress?.total ?? "?"} 个（
              {syncProgress && syncProgress.total > 0 ? syncProgress.percent.toFixed(1) : "0.0"}%）
            </span>
          </div>
          {syncProgress?.currentFile ? (
            <p className="truncate rounded-md bg-muted px-2 py-1.5 font-mono text-xs text-muted-foreground" title={syncProgress.currentFile}>
              {syncProgress.currentFile}
            </p>
          ) : null}
        </div>
      )}

      {/* 撤销进行中：进度条 */}
      {undoing && (
        <div className="space-y-2">
          <div className="flex items-center justify-between gap-4">
            <span className="text-sm font-medium">正在撤销同步（删除复制产生的文件）…</span>
            <Button variant="destructive" size="sm" onClick={onStopUndo}>
              停止撤销
            </Button>
          </div>
          <Progress value={undoProgress?.percent ?? 0} />
          <div className="flex items-center justify-between text-xs text-muted-foreground">
            <span>
              已删除 {undoProgress?.removed ?? 0} / {undoProgress?.total ?? "?"} 个（
              {undoProgress && undoProgress.total > 0 ? undoProgress.percent.toFixed(1) : "0.0"}%）
            </span>
          </div>
          {undoProgress?.currentFile ? (
            <p className="truncate rounded-md bg-muted px-2 py-1.5 font-mono text-xs text-muted-foreground" title={undoProgress.currentFile}>
              {undoProgress.currentFile}
            </p>
          ) : null}
        </div>
      )}

      {/* 同步完成：结果摘要 */}
      {!syncing && syncResult && (
        <div className="space-y-2">
          <div
            className={cn(
              "rounded-lg border px-3 py-2 text-sm",
              syncResult.failed > 0
                ? "border-amber-500/40 bg-amber-500/10 text-amber-600 dark:text-amber-400"
                : "border-border bg-muted text-foreground"
            )}
          >
            {syncResult.cancelled ? "⏹ " : "✅ "}
            「{syncResult.actionLabel}」{syncResult.cancelled ? "已停止" : "完成"}：复制{" "}
            <span className="font-semibold">{syncResult.copied.toLocaleString()}</span> 个 · 冲突跳过{" "}
            <span className="font-semibold">{syncResult.skipped.toLocaleString()}</span> 个 · 失败{" "}
            <span className="font-semibold">{syncResult.failed.toLocaleString()}</span> 个 · 耗时{" "}
            {formatDuration(syncResult.durationMs)}
          </div>
          {(syncResult.conflicts.length > 0 || syncResult.errors.length > 0) && (
            <div className="space-y-1">
              <button
                className="text-xs text-muted-foreground underline-offset-2 hover:text-foreground hover:underline"
                onClick={() => setShowErrors(!showErrors)}
              >
                {showErrors ? "收起明细" : `查看明细（冲突 ${syncResult.conflicts.length} · 失败 ${syncResult.errors.length}）`}
              </button>
              {showErrors && (
                <div className="max-h-40 space-y-1 overflow-y-auto rounded-lg border bg-muted/50 p-2">
                  {syncResult.errors.slice(0, 500).map((e, i) => (
                    <p key={`e${i}`} className="truncate font-mono text-xs text-destructive" title={`${e.path} — ${e.error}`}>
                      {e.path} — {e.error}
                    </p>
                  ))}
                  {syncResult.conflicts.slice(0, 500).map((p, i) => (
                    <p key={`c${i}`} className="truncate font-mono text-xs text-muted-foreground" title={`${p}（目标已存在同名文件，未覆盖）`}>
                      [冲突] {p}
                    </p>
                  ))}
                </div>
              )}
            </div>
          )}
          <p className="text-xs text-muted-foreground">
            上方结果列表为同步前的快照，建议点击「开始比对」重新比对以验证一致性
          </p>
        </div>
      )}

      {/* 撤销完成：结果摘要 */}
      {!undoing && undoResult && (
        <div
          className={cn(
            "rounded-lg border px-3 py-2 text-sm",
            undoResult.failed > 0
              ? "border-amber-500/40 bg-amber-500/10 text-amber-600 dark:text-amber-400"
              : "border-border bg-muted text-foreground"
          )}
        >
          {undoResult.cancelled ? "⏹ " : "✅ "}
          「撤销同步」{undoResult.cancelled ? "已停止" : "完成"}：删除{" "}
          <span className="font-semibold">{undoResult.removed.toLocaleString()}</span> 个 · 已不存在{" "}
          <span className="font-semibold">{undoResult.missing.toLocaleString()}</span> 个 · 已修改保留{" "}
          <span className="font-semibold">{undoResult.modified.toLocaleString()}</span> 个 · 失败{" "}
          <span className="font-semibold">{undoResult.failed.toLocaleString()}</span> 个 · 清理空目录{" "}
          <span className="font-semibold">{undoResult.removedDirs.toLocaleString()}</span> 个 · 耗时{" "}
          {formatDuration(undoResult.durationMs)}
          {undoResult.errors.length > 0 && (
            <div className="mt-1 max-h-24 space-y-0.5 overflow-y-auto">
              {undoResult.errors.slice(0, 500).map((e, i) => (
                <p key={i} className="truncate font-mono text-xs text-destructive" title={`${e.path} — ${e.error}`}>
                  {e.path} — {e.error}
                </p>
              ))}
            </div>
          )}
        </div>
      )}

      {/* 操作按钮（同步/撤销进行中禁用） */}
      <div className="flex flex-wrap items-center gap-2">
        <Button size="sm" disabled={busy || !hasUnique} onClick={() => onAction("unify")} title="将 A 独有复制到 B，同时将 B 独有复制到 A">
          🔁 一键一致化（共 {(onlyACount + onlyBCount).toLocaleString()} 个）
        </Button>
        <Button size="sm" variant="outline" disabled={busy || onlyACount === 0} onClick={() => onAction("aToB")}>
          A 独有 → B（{onlyACount.toLocaleString()} 个）
        </Button>
        <Button size="sm" variant="outline" disabled={busy || onlyBCount === 0} onClick={() => onAction("bToA")}>
          B 独有 → A（{onlyBCount.toLocaleString()} 个）
        </Button>
        <Button
          size="sm"
          variant="outline"
          disabled={busy || undoable === 0}
          onClick={onUndo}
          title="删除本次比对会话内、历次同步复制到对侧的文件（大小被修改过的会保留）"
          className={cn(undoable > 0 && !busy && "border-amber-500/50 text-amber-600 hover:text-amber-600 dark:text-amber-400")}
        >
          ↩ 撤销同步{undoable > 0 ? `（${undoable.toLocaleString()} 个）` : ""}
        </Button>
        <Button size="sm" variant="ghost" disabled={busy} onClick={onIgnore}>
          不做处理
        </Button>
      </div>
    </section>
  );
}
