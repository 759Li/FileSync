/** 后端 compare-result 事件的数据结构（与 Rust 侧 serde 定义对应） */

export interface FileInfo {
  path: string;
  size: number;
}

/** 第一组：内容相同（哈希相同，文件名可能不同）的重复文件组 */
export interface DuplicateGroup {
  hash: string;
  size: number;
  filesA: FileInfo[];
  filesB: FileInfo[];
}

/** 第二/三组：仅单侧存在的独有文件（hash 为 null 表示经大小优化跳过了哈希计算） */
export interface UniqueFile {
  path: string;
  size: number;
  hash: string | null;
}

export interface ErrorItem {
  path: string;
  error: string;
}

export interface CompareStats {
  totalA: number;
  totalB: number;
  hashed: number;
  skippedBySize: number;
  errorCount: number;
  cancelledCount: number;
  durationMs: number;
  cancelled: boolean;
}

export interface CompareResult {
  duplicates: DuplicateGroup[];
  onlyA: UniqueFile[];
  onlyB: UniqueFile[];
  errors: ErrorItem[];
  stats: CompareStats;
}

/** 后端 compare-progress 事件的数据结构 */
export interface ProgressPayload {
  stage: "enumerating" | "hashing" | string;
  side: string;
  scanned: number;
  total: number;
  currentFile: string;
  /** 0-100，-1 表示未知（枚举阶段） */
  percent: number;
}

/* ---------------- 重复文件扫描 ---------------- */

/** 一组内容完全相同的文件（哈希相同，文件名可不同） */
export interface DupGroup {
  /** 组号（从 1 开始，按可释放空间降序） */
  id: number;
  hash: string;
  /** 组内每个文件的大小（字节，组内一致） */
  size: number;
  /** 组内文件（按路径升序，首个为建议保留的副本） */
  files: FileInfo[];
  /** 组内仅保留 1 个副本时可释放的空间（字节） */
  reclaimable: number;
}

export interface DupScanStats {
  totalFiles: number;
  /** 实际计算了哈希的文件数（同大小候选） */
  hashed: number;
  /** 大小唯一、无需哈希的文件数（预筛跳过） */
  skippedBySize: number;
  dupGroupCount: number;
  dupFileCount: number;
  /** 每组保留一个副本后可释放的总空间（字节） */
  reclaimableBytes: number;
  errorCount: number;
  cancelledCount: number;
  durationMs: number;
  cancelled: boolean;
}

/** 后端 dup-scan-result 事件的数据结构 */
export interface DupScanResult {
  /** 扫描根目录 */
  root: string;
  groups: DupGroup[];
  errors: ErrorItem[];
  stats: DupScanStats;
}

/** 后端 dup-scan-progress 事件的数据结构 */
export interface DupScanProgressPayload {
  stage: "enumerating" | "hashing" | string;
  scanned: number;
  total: number;
  currentFile: string;
  /** 0-100，-1 表示未知（枚举阶段） */
  percent: number;
}

/* ---------------- 重复文件删除与回收站（可撤销） ---------------- */

/** 提交给后端的待删除文件（size 为扫描时记录的大小，用于删除前校验） */
export interface DupFileRef {
  path: string;
  size: number;
}

/** 后端 dup-delete-progress 事件的数据结构（删除 / 撤销 / 清空三类任务共用） */
export interface DupDeleteProgressPayload {
  /** deleting 移入回收站 | restoring 恢复到原位置 | purging 彻底清空 */
  phase: "deleting" | "restoring" | "purging" | string;
  /** 已处理的文件数（含跳过 / 失败） */
  processed: number;
  total: number;
  currentFile: string;
  /** 0-100 */
  percent: number;
}

/** 后端 dup-delete-result 事件的数据结构 */
export interface DupDeleteResult {
  deleted: number;
  freedBytes: number;
  skipped: number;
  failed: number;
  cancelled: boolean;
  durationMs: number;
  errors: ErrorItem[];
  /** 本次删除产生的回收站批次号（可一键撤销）；无文件被删除时为 null */
  batchId: string | null;
}

/** 后端 dup-undo-result 事件的数据结构 */
export interface DupUndoResult {
  batchId: string;
  /** 成功恢复到原位置的文件数 */
  restored: number;
  /** 原位置已存在同名文件而跳过的数量（不覆盖） */
  conflicts: number;
  failed: number;
  cancelled: boolean;
  durationMs: number;
  errors: ErrorItem[];
}

/** 后端 dup-purge-result 事件的数据结构 */
export interface DupPurgeResult {
  purgedFiles: number;
  purgedBytes: number;
  failed: number;
  durationMs: number;
  errors: ErrorItem[];
}

/** 回收站中的一条删除批次（可撤销；最新在前） */
export interface DeleteBatchSummary {
  id: string;
  /** 创建时间（Unix 毫秒） */
  createdAt: number;
  /** 删除时的扫描根目录 */
  root: string;
  fileCount: number;
  totalBytes: number;
}

/** 同步动作：unify 一键一致化 | aToB A独有→B | bToA B独有→A | selected 一致化勾选文件 */
export type SyncAction = "unify" | "aToB" | "bToA" | "selected";

/** 后端 sync-progress 事件的数据结构 */
export interface SyncProgressPayload {
  copied: number;
  total: number;
  currentFile: string;
  /** 0-100 */
  percent: number;
}

/** 后端 sync-result 事件的数据结构 */
export interface SyncResult {
  action: SyncAction;
  actionLabel: string;
  copied: number;
  /** 冲突跳过数（目标侧已存在同名文件） */
  skipped: number;
  failed: number;
  cancelled: boolean;
  durationMs: number;
  errors: ErrorItem[];
  conflicts: string[];
  /** 本次同步后累计可撤销的复制文件数 */
  undoable: number;
}

/** 后端 undo-progress 事件的数据结构 */
export interface UndoProgressPayload {
  removed: number;
  total: number;
  currentFile: string;
  /** 0-100 */
  percent: number;
}

/** 后端 undo-result 事件的数据结构 */
export interface UndoResult {
  /** 成功删除（撤销）数 */
  removed: number;
  /** 目标文件已不存在，无需撤销 */
  missing: number;
  /** 目标文件大小与复制时不一致（可能已被修改），保留未删 */
  modified: number;
  /** 删除失败数（仍保留在撤销记录中，可重试） */
  failed: number;
  cancelled: boolean;
  /** 顺带清理的空目录数 */
  removedDirs: number;
  durationMs: number;
  /** 撤销后仍可撤销的文件数 */
  remaining: number;
  errors: ErrorItem[];
}
