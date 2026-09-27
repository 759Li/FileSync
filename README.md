# FileSync 文件夹比对工具

基于 Tauri 2 + React + Rust 的桌面应用：对两个文件夹进行深度内容比对，按 **SHA256 哈希**识别重复文件，不依赖文件名与修改时间。比对完成后可选择**一键一致化**等同步操作，让两侧文件夹内容保持一致。适用于备份核对、迁移验证、双向同步、去重清理等场景。

## 功能特性

- **内容级比对**：按 SHA256 全量哈希分组，同名不同内容、异名同内容均可正确识别
- **大小优先优化**：先比对两侧文件大小集合，大小无匹配的文件直接跳过哈希计算，速度大幅提升
- **超大文件友好**：4MB 分块读取计算哈希，内存占用稳定；超大文件不会撑爆内存
- **多核并行**：基于 rayon 的并行哈希计算，线程数等于 CPU 核心数
- **实时进度**：阶段 / 侧别 / 计数 / 百分比 / 当前文件实时推送（150ms 心跳），UI 不卡顿
- **随时可取消**：取消后输出已完成部分的结果并标记
- **四类结果视图**：重复（A∩B）/ 仅在 A / 仅在 B / 错误列表，Tab 切换
- **比对后同步操作**：比对完成后可选择后续动作，将单侧独有文件复制到另一侧
  - **一键一致化**：A 独有 → B 且 B 独有 → A，双向互补复制，使两侧内容完全一致
  - **A 独有 → B** / **B 独有 → A**：按需单向复制
  - **不做处理**：仅查看结果，关闭操作面板（可从统计栏随时重新打开）
  - 同步过程显示实时进度、可随时停止；复制保留相对目录结构，自动创建目标子目录
  - **安全不覆盖**：目标侧已存在同名（不同内容）文件时自动跳过并计入「冲突」，绝不覆盖已有数据；完成后展示复制 / 冲突 / 失败统计与明细
- **CSV 导出**：UTF-8 BOM 编码，Excel 打开不乱码
- **细节交互**：双击行打开资源管理器并定位选中文件；一键复制路径 / 哈希；浅色 / 深色主题
- **异常容错**：遍历 / 读取失败的文件计入错误列表，不中断整体任务

## 技术栈

| 层 | 技术 |
|---|---|
| 前端 | React 18 + TypeScript + Vite 5 + Tailwind CSS |
| 后端 | Rust（walkdir / sha2 / rayon） |
| 框架 | Tauri 2（tauri-plugin-dialog） |

## 环境要求

- Node.js 18+
- Rust 工具链（MSVC 目标）
- VS Build Tools 2022（勾选「C++ 桌面开发」）

```powershell
# 安装 Rust 与 VS Build Tools（如尚未安装）
winget install Rustlang.Rustup
winget install Microsoft.VisualStudio.2022.BuildTools
```

## 快速开始

```powershell
git clone https://gitcode.com/lzb759/FileSync.git
cd FileSync
npm install        # 安装前端依赖
npm run tauri dev  # 启动开发模式（热重载）
```

## 构建打包

```powershell
npm run tauri build
```

产物位置：

- 安装包：`src-tauri\target\release\bundle\nsis\FileSync_<版本号>_x64-setup.exe`
- 绿色单文件：`src-tauri\target\release\filesync.exe`

> 说明：NSIS 工具链缓存在 `%LOCALAPPDATA%\tauri\NSIS`，首次打包需从 GitHub 下载；若无法直连 GitHub，可在原始链接前拼接镜像前缀（如 `https://ghproxy.net/`）下载 `nsis-3.11.zip` 与 `nsis_tauri_utils.dll`，校验 SHA1 后放入缓存目录（DLL 须位于 `Plugins\x86-unicode\additional\` 子目录），之后打包无需联网。

## 使用说明

1. 启动应用后，分别选择需要比对的文件夹 A 与 B
2. 点击「开始比对」，等待扫描与哈希计算完成
3. 在四个结果 Tab 中查看：
   - **重复**：两侧内容完全一致的文件（按哈希分组，展示各自路径）
   - **仅在 A**：A 独有的文件
   - **仅在 B**：B 独有的文件
   - **错误**：无法读取 / 遍历失败的条目
4. 比对完整完成后，结果上方会出现「后续操作」面板，选择一种同步动作：
   - **一键一致化**：把 A 独有文件复制到 B，同时把 B 独有文件复制到 A（推荐用于双向备份对齐）
   - **A 独有 → B** / **B 独有 → A**：仅单向复制某一侧的独有文件
   - **不做处理**：关闭面板；如需再次操作，点击统计栏右侧「同步操作」重新打开
   - 同步时显示实时进度并可「停止同步」；完成后展示复制 / 冲突跳过 / 失败统计，可展开查看明细
   - 目标侧已存在同名（不同内容）文件会作为冲突跳过，**不会覆盖**
5. 可随时点击「停止」中断比对任务，已完成的比对结果仍然保留（被中断的比对不提供同步操作）
6. 点击「导出 CSV」保存结果，双击任意条目可在资源管理器中定位该文件
7. 同步完成后，结果列表仍为同步前快照，建议重新比对一次以验证两侧已一致

## 项目结构

```
FileSync/
├── src/                          # 前端：UI、参数输入、结果展示
│   ├── App.tsx                   # 主界面：事件订阅、开始/停止/导出、同步接线
│   ├── types.ts                  # 与 Rust serde 对应的结果/同步类型
│   └── components/
│       ├── FolderPicker.tsx      # 文件夹选择
│       ├── ProgressPanel.tsx     # 比对进度条 + 当前文件 + 停止
│       ├── SyncPanel.tsx         # 比对后同步操作面板（一致化/单向复制/不做处理）
│       └── ResultsView.tsx       # 4 Tab 结果视图
└── src-tauri/                    # 后端：全部文件 IO / 哈希 / 遍历 / 同步复制
    └── src/
        ├── scanner.rs            # 递归枚举、4MB 分块 SHA256、rayon 并行
        ├── compare.rs            # A∩B / A-B / B-A 分类
        ├── sync.rs               # 同步动作：双向一致化 / 单向复制（冲突跳过）
        ├── commands.rs           # start/stop_compare、apply_sync_action/stop_sync、export_csv、open_in_explorer
        ├── csv_export.rs         # UTF-8 BOM CSV 导出
        └── state.rs              # 取消/运行标志 + 结果与根目录缓存
```

## 许可证

本项目暂未指定开源许可证，所有权利默认保留。
