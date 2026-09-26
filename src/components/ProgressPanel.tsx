import { invoke } from "@tauri-apps/api/core";
import { Button } from "./ui/button";
import { Progress } from "./ui/progress";
import type { ProgressPayload } from "../types";
import { cn } from "../lib/utils";

/** 打开文件所在文件夹（资源管理器中定位并选中） */
export function openInExplorer(path: string) {
  invoke("open_in_explorer", { path }).catch(console.error);
}

interface ProgressPanelProps {
  progress: ProgressPayload | null;
  running: boolean;
  onStop: () => void;
}

/** 进度区域：进度条、计数、当前文件路径、停止按钮 */
export function ProgressPanel({ progress, running, onStop }: ProgressPanelProps) {
  if (!running && !progress) return null;

  const isEnum = progress?.stage === "enumerating";
  const stageText = isEnum
    ? `正在枚举文件夹 ${progress?.side ?? ""}（已发现 ${progress?.scanned ?? 0} 个文件）`
    : `正在计算哈希 · 文件夹 ${progress?.side ?? ""}`;

  return (
    <div className="space-y-3 rounded-xl border bg-card p-4 shadow-sm">
      <div className="flex items-center justify-between gap-4">
        <span className="text-sm font-medium">{stageText}</span>
        {running && (
          <Button variant="destructive" size="sm" onClick={onStop}>
            停止扫描
          </Button>
        )}
      </div>
      <Progress value={progress?.percent ?? -1} />
      <div className="flex items-center justify-between text-xs text-muted-foreground">
        <span>
          {isEnum
            ? "扫描文件中…"
            : `已处理 ${progress?.scanned ?? 0} / ${progress?.total ?? "?"} 个文件（${
                progress && progress.percent >= 0 ? progress.percent.toFixed(1) : "--"
              }%）`}
        </span>
        <span>多线程并行哈希中，不阻塞界面</span>
      </div>
      {progress?.currentFile ? (
        <p
          className={cn("truncate rounded-md bg-muted px-2 py-1.5 font-mono text-xs text-muted-foreground")}
          title={progress.currentFile}
        >
          {progress.currentFile}
        </p>
      ) : null}
    </div>
  );
}
