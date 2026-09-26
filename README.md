# FileSync 文件夹比对工具

基于 Tauri 2 + React + Rust 的桌面应用：对两个文件夹进行深度内容比对，按 **SHA256 哈希**识别重复文件，不依赖文件名与修改时间。适用于备份核对、迁移验证、去重清理等场景。

## 功能特性

- **内容级比对**：按 SHA256 全量哈希分组，同名不同内容、异名同内容均可正确识别
- **大小优先优化**：先比对两侧文件大小集合，大小无匹配的文件直接跳过哈希计算，速度大幅提升
- **超大文件友好**：4MB 分块读取计算哈希，内存占用稳定；超大文件不会撑爆内存
- **多核并行**：基于 rayon 的并行哈希计算，线程数等于 CPU 核心数
- **实时进度**：阶段 / 侧别 / 计数 / 百分比 / 当前文件实时推送（150ms 心跳），UI 不卡顿
- **随时可取消**：取消后输出已完成部分的结果并标记
- **四类结果视图**：重复（A∩B）/ 仅在 A / 仅在 B / 错误列表，Tab 切换
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

- 安装包：`src-tauri\target\release\bundle\nsis\FileSync_0.1.0_x64-setup.exe`
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
4. 可随时点击「停止」中断任务，已完成的比对结果仍然保留
5. 点击「导出 CSV」保存结果，双击任意条目可在资源管理器中定位该文件

## 项目结构

```
FileSync/
├── src/                          # 前端：UI、参数输入、结果展示
│   ├── App.tsx                   # 主界面：事件订阅、开始/停止/导出
│   ├── types.ts                  # 与 Rust serde 对应的结果类型
│   └── components/
│       ├── FolderPicker.tsx      # 文件夹选择
│       ├── ProgressPanel.tsx     # 进度条 + 当前文件 + 停止
│       └── ResultsView.tsx       # 4 Tab 结果视图
└── src-tauri/                    # 后端：全部文件 IO / 哈希 / 遍历
    └── src/
        ├── scanner.rs            # 递归枚举、4MB 分块 SHA256、rayon 并行
        ├── compare.rs            # A∩B / A-B / B-A 分类
        ├── commands.rs           # start/stop_compare、export_csv、open_in_explorer
        ├── csv_export.rs         # UTF-8 BOM CSV 导出
        └── state.rs              # 取消/运行标志 + 结果缓存
```

## 许可证

本项目暂未指定开源许可证，所有权利默认保留。
