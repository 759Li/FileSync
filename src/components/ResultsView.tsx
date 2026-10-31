import { useEffect, useMemo, useRef, useState } from "react";
import type { CompareResult, DuplicateGroup, UniqueFile } from "../types";
import { copyText } from "../lib/utils";
import { formatBytes } from "../lib/format";
import { openInExplorer } from "./ProgressPanel";
import { cn } from "../lib/utils";

/* ---------------- Tab 页定义 ---------------- */

type TabKey = "dup" | "onlyA" | "onlyB" | "err";

interface ResultsViewProps {
  result: CompareResult;
  /** 「仅 A 独有 / 仅 B 独有」中勾选待一致化的文件路径 */
  selectedA: Set<string>;
  selectedB: Set<string>;
  onToggleSelected: (side: "A" | "B", path: string) => void;
  onSelectAll: (side: "A" | "B", checked: boolean) => void;
}

/** 结果展示：4 个 Tab（重复 / 仅A / 仅B / 错误），表格 + 分组 */
export function ResultsView({
  result,
  selectedA,
  selectedB,
  onToggleSelected,
  onSelectAll,
}: ResultsViewProps) {
  const [tab, setTab] = useState<TabKey>("dup");

  const tabs = useMemo(
    () => [
      { key: "dup" as TabKey, label: "内容重复", count: result.duplicates.length, unit: "组" },
      { key: "onlyA" as TabKey, label: "仅 A 独有", count: result.onlyA.length, unit: "个" },
      { key: "onlyB" as TabKey, label: "仅 B 独有", count: result.onlyB.length, unit: "个" },
      { key: "err" as TabKey, label: "错误", count: result.errors.length, unit: "条" },
    ],
    [result]
  );

  return (
    <div className="space-y-3">
      {/* Tab 栏 */}
      <div className="flex gap-1 rounded-lg bg-muted p-1">
        {tabs.map((t) => (
          <button
            key={t.key}
            onClick={() => setTab(t.key)}
            className={cn(
              "flex flex-1 items-center justify-center gap-1.5 rounded-md px-3 py-1.5 text-sm font-medium transition-colors",
              tab === t.key ? "bg-background shadow text-foreground" : "text-muted-foreground hover:text-foreground"
            )}
          >
            {t.label}
            <span
              className={cn(
                "rounded-full px-1.5 text-xs",
                t.count > 0 ? "bg-primary text-primary-foreground" : "bg-secondary text-muted-foreground"
              )}
            >
              {t.count}
              {t.unit}
            </span>
          </button>
        ))}
      </div>

      {/* 内容区 */}
      <div className="max-h-[calc(100vh-460px)] min-h-[200px] overflow-y-auto rounded-xl border bg-card shadow-sm">
        {tab === "dup" && <DuplicatesTab groups={result.duplicates} />}
        {tab === "onlyA" && (
          <UniqueTab
            files={result.onlyA}
            side="A"
            selected={selectedA}
            onToggle={(path) => onToggleSelected("A", path)}
            onSelectAll={(checked) => onSelectAll("A", checked)}
          />
        )}
        {tab === "onlyB" && (
          <UniqueTab
            files={result.onlyB}
            side="B"
            selected={selectedB}
            onToggle={(path) => onToggleSelected("B", path)}
            onSelectAll={(checked) => onSelectAll("B", checked)}
          />
        )}
        {tab === "err" && <ErrorsTab result={result} />}
      </div>
    </div>
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

/* ---------------- 第一组：内容重复（按哈希分组） ---------------- */

function DuplicatesTab({ groups }: { groups: DuplicateGroup[] }) {
  if (groups.length === 0) return <Empty text="未找到内容重复的文件" />;
  return (
    <div className="divide-y">
      {groups.map((g, i) => (
        <div key={g.hash} className="p-3">
          {/* 组头 */}
          <div className="mb-2 flex flex-wrap items-center gap-2 text-xs">
            <span className="rounded bg-primary px-1.5 py-0.5 font-medium text-primary-foreground">第 {i + 1} 组</span>
            <span className="text-muted-foreground">{formatBytes(g.size)}</span>
            <code className="rounded bg-muted px-1.5 py-0.5 font-mono" title={g.hash}>
              {g.hash.slice(0, 16)}…
            </code>
            <CopyBtn text={g.hash} label="完整哈希" />
          </div>
          {/* 组内文件 */}
          <div className="space-y-0.5">
            {g.filesA.map((f) => (
              <FileRow key={f.path} path={f.path} size={f.size} side="A" />
            ))}
            {g.filesB.map((f) => (
              <FileRow key={f.path} path={f.path} size={f.size} side="B" />
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}

function FileRow({ path, size, side }: { path: string; size: number; side?: "A" | "B" }) {
  return (
    <div
      className="group/row flex items-center gap-2 rounded-md px-2 py-1 hover:bg-accent"
      onDoubleClick={() => openInExplorer(path)}
      title="双击打开所在文件夹"
    >
      {side && (
        <span
          className={cn(
            "shrink-0 rounded px-1.5 py-0.5 text-xs font-medium",
            side === "A" ? "bg-blue-500/15 text-blue-600 dark:text-blue-400" : "bg-violet-500/15 text-violet-600 dark:text-violet-400"
          )}
        >
          {side}
        </span>
      )}
      <span className="truncate font-mono text-xs" title={path}>
        {path}
      </span>
      <span className="ml-auto shrink-0 text-xs text-muted-foreground">{formatBytes(size)}</span>
      <CopyBtn text={path} label="文件路径" />
    </div>
  );
}

/* ---------------- 第二/三组：单侧独有文件（可勾选一致化） ---------------- */

interface UniqueTabProps {
  files: UniqueFile[];
  side: "A" | "B";
  /** 已勾选的文件路径 */
  selected: Set<string>;
  onToggle: (path: string) => void;
  onSelectAll: (checked: boolean) => void;
}

function UniqueTab({ files, side, selected, onToggle, onSelectAll }: UniqueTabProps) {
  if (files.length === 0) return <Empty text={`无仅文件夹 ${side} 独有的文件`} />;
  const selectedCount = files.reduce((n, f) => n + (selected.has(f.path) ? 1 : 0), 0);
  const allChecked = selectedCount === files.length;
  const someChecked = selectedCount > 0 && !allChecked;

  return (
    <table className="w-full text-sm">
      <thead className="sticky top-0 bg-muted/80 text-left text-xs text-muted-foreground backdrop-blur">
        {/* 勾选工具条 */}
        <tr>
          <th colSpan={4} className="px-3 pb-0 pt-2">
            <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
              <label className="flex cursor-pointer items-center gap-1.5 text-foreground">
                <SelectAllCheckbox checked={allChecked} indeterminate={someChecked} onChange={onSelectAll} />
                全选
              </label>
              <span>
                已选 <span className="font-medium text-foreground">{selectedCount.toLocaleString()}</span> /{" "}
                {files.length.toLocaleString()} 个
              </span>
              <span>勾选后点击上方「✅ 一致化选中」复制到文件夹 {side === "A" ? "B" : "A"}</span>
            </div>
          </th>
        </tr>
        <tr>
          <th className="w-8 px-2 py-2" />
          <th className="px-3 py-2 font-medium">文件路径</th>
          <th className="w-24 px-2 py-2 font-medium">大小</th>
          <th className="w-44 px-2 py-2 font-medium">哈希值</th>
        </tr>
      </thead>
      <tbody className="divide-y">
        {files.map((f) => {
          const checked = selected.has(f.path);
          return (
            <tr
              key={f.path}
              className={cn("group/row cursor-default hover:bg-accent", checked && "bg-primary/5")}
              onDoubleClick={() => openInExplorer(f.path)}
              title="双击打开所在文件夹"
            >
              <td className="px-2 py-1.5">
                <input
                  type="checkbox"
                  checked={checked}
                  onChange={() => onToggle(f.path)}
                  className="h-3.5 w-3.5 shrink-0 accent-primary"
                  title={checked ? "已勾选（将复制到对侧）" : "勾选以一致化此文件"}
                />
              </td>
              <td className="max-w-0 truncate px-3 py-1.5 font-mono text-xs" title={f.path}>
                {f.path}
                <CopyBtn text={f.path} label="文件路径" />
              </td>
              <td className="whitespace-nowrap px-2 py-1.5 text-xs text-muted-foreground">{formatBytes(f.size)}</td>
              <td className="px-2 py-1.5">
                {f.hash ? (
                  <div className="flex items-center gap-1">
                    <code className="truncate font-mono text-xs text-muted-foreground" title={f.hash}>
                      {f.hash.slice(0, 16)}…
                    </code>
                    <CopyBtn text={f.hash} label="哈希值" />
                  </div>
                ) : (
                  <span className="text-xs text-muted-foreground/60" title="大小优化：另一侧无同大小文件，无需计算哈希">
                    （大小不同已跳过）
                  </span>
                )}
              </td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

/** 全选复选框（半选状态只能通过 DOM 属性设置） */
function SelectAllCheckbox({
  checked,
  indeterminate,
  onChange,
}: {
  checked: boolean;
  indeterminate: boolean;
  onChange: (checked: boolean) => void;
}) {
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (ref.current) ref.current.indeterminate = indeterminate;
  }, [indeterminate]);
  return (
    <input
      ref={ref}
      type="checkbox"
      checked={checked}
      onChange={(e) => onChange(e.target.checked)}
      className="h-3.5 w-3.5 shrink-0 accent-primary"
    />
  );
}

/* ---------------- 错误列表 ---------------- */

function ErrorsTab({ result }: { result: CompareResult }) {
  if (result.errors.length === 0) return <Empty text="没有读取失败的文件" />;
  return (
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
  );
}

function Empty({ text }: { text: string }) {
  return <div className="flex h-40 items-center justify-center text-sm text-muted-foreground">{text}</div>;
}
