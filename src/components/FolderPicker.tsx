import { open } from "@tauri-apps/plugin-dialog";
import { Button } from "./ui/button";

interface FolderPickerProps {
  label: string;
  value: string;
  onChange: (path: string) => void;
}

/** 文件夹选择输入框 + 【选择文件夹】按钮 */
export function FolderPicker({ label, value, onChange }: FolderPickerProps) {
  const pick = async () => {
    const dir = await open({ directory: true, title: `选择${label}` });
    if (typeof dir === "string") onChange(dir);
  };

  return (
    <div className="flex items-center gap-2 rounded-lg border border-input bg-background px-3 py-2 shadow-sm">
      <span className="w-16 shrink-0 text-sm font-medium text-muted-foreground">{label}</span>
      <input
        type="text"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={`请选择或输入${label}路径…`}
        className="h-7 w-full bg-transparent text-sm outline-none placeholder:text-muted-foreground/60"
      />
      <Button variant="outline" size="sm" onClick={pick} className="shrink-0">
        选择文件夹
      </Button>
    </div>
  );
}
