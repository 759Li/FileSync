import { cn } from "../../lib/utils";

interface ProgressProps {
  /** 0-100；传 -1 表示不确定进度（枚举阶段） */
  value: number;
  className?: string;
}

/** shadcn/ui 风格进度条 */
export function Progress({ value, className }: ProgressProps) {
  const indeterminate = value < 0;
  return (
    <div className={cn("relative h-2 w-full overflow-hidden rounded-full bg-secondary", className)}>
      {indeterminate ? (
        <div className="h-full w-full origin-left animate-pulse bg-primary" />
      ) : (
        <div
          className="h-full bg-primary transition-all duration-200"
          style={{ width: `${Math.min(100, Math.max(0, value))}%` }}
        />
      )}
    </div>
  );
}
