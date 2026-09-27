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

/** 同步动作：unify 一键一致化 | aToB A独有→B | bToA B独有→A */
export type SyncAction = "unify" | "aToB" | "bToA";

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
}
