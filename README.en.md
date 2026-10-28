# FileSync

**Content-hash folder comparison, synchronization, and duplicate cleanup for Windows**

**English** | [简体中文](README.md)

`v0.3.0` · Windows 10/11 x64 · [GPL-3.0](LICENSE) · Tauri 2 + React + Rust

FileSync is a local file-management tool for Windows. Instead of relying on file names or timestamps, it identifies identical files by **SHA-256 content hash**, helping you verify backups, validate migrations, keep two folders in sync, and reclaim disk space from duplicates.

All hashing and file operations run locally — nothing is uploaded.

> The application UI is currently in Simplified Chinese. UI labels are translated in parentheses below where helpful.

## Table of Contents

- [Core Capabilities](#core-capabilities)
- [Screenshots](#screenshots)
- [Features](#features)
- [Safety by Design](#safety-by-design)
- [Requirements](#requirements)
- [Download and Install](#download-and-install)
- [Run from Source](#run-from-source)
- [Build and Package](#build-and-package)
- [Usage](#usage)
- [FAQ](#faq)
- [Tech Stack](#tech-stack)
- [Project Structure](#project-structure)
- [Contributing](#contributing)
- [Version History](#version-history)
- [License](#license)

## Core Capabilities

| Mode | Description |
|---|---|
| **Folder comparison & sync** | Deep-compare two folders by content, list identical and one-sided files, then copy one-sided files to the other side in one click |
| **Duplicate scan & cleanup** | Recursively scan a single folder for duplicate files and move selected copies into an App-managed, undoable trash |

Typical use cases: checking whether a backup is complete, verifying files after migration, aligning two copies of a dataset, and cleaning up disk space.

## Screenshots

### Folder Comparison

![Folder comparison and sync](docs/screenshots/folder-compare.png)

### Duplicate Scan

![Duplicate scan and cleanup](docs/screenshots/duplicate-scan.png)

## Features

### Folder Comparison & Sync

- **Content-level comparison**: Files are grouped by full SHA-256 hash, so same-name/different-content and different-name/same-content files are both handled correctly
- **Size prefilter**: Files whose size does not exist on the other side skip hashing entirely (their content cannot match)
- **Large-file friendly**: 4 MB chunked reads keep memory usage stable, even for very large files
- **Multicore hashing**: rayon parallelizes hashing across all CPU logical cores
- **Live progress**: Current stage, side, counters, percentage, and current file are pushed to the UI while the interface stays responsive
- **Cancel anytime**: Stopping preserves the results completed so far and marks the run as cancelled
- **Four result views**: Duplicates (A∩B), Only in A, Only in B, and Errors, switchable via tabs
- **Post-comparison sync**:

| Action | Behavior |
|---|---|
| Make Both Sides Identical | Copy A-only files to B and B-only files to A |
| A-only → B | Copy only A's unique files to B |
| B-only → A | Copy only B's unique files to A |
| Do Nothing | Just review results; reopen the panel from the stats bar later |

- Sync preserves relative directory structure and creates missing target directories; progress is shown and can be stopped
- **Never overwrites**: If the target already has a file with the same name but different content, it is skipped and counted as a conflict. Existing data is never overwritten; a summary shows copied / conflicted / failed counts and details
- **Undoable sync**: Revert files copied during the current comparison session
  - Before deletion, each target file's size is checked against the size recorded at copy time; modified files are kept to prevent accidental deletion
  - Missing target files count as already undone; directories left empty are cleaned up automatically (non-empty directories are untouched)
  - Progress is shown and can be stopped; remaining entries stay in the record and can be undone later
  - Undo records live in memory and are cleared when a new comparison starts or the app exits

### Duplicate Scan & Cleanup

- Recursively walks all subfolders; unreadable / failed entries are collected in an error list without aborting the scan
- **Two-stage confirmation**: Files are first grouped by size; files with a unique size skip hashing entirely (prefilter). Only same-size files are hashed with SHA-256 and grouped by hash; a group with at least 2 files is a duplicate group — **duplicates are never inferred from file names**
- Group list shows group number, per-file size, file count, reclaimable space, and full paths; double-click a row to reveal it in Explorer
- **Cleanup helpers**: By default every copy except the first is selected (the first file, sorted by path, is the suggested keeper). Select all / none, "Keep only this file", or copy all selected paths (with total size) in one click
- Groups are sorted by reclaimable space so the biggest wins come first
- **One-click delete (undoable)**: Selected copies are moved into the App-managed trash (`<App data dir>/filesync-trash/`, not the Windows Recycle Bin)
  - Triple validation before each deletion: the file must be inside the scan root, must still exist, and its size must match the size recorded during the scan
  - Live progress and stop support; processed files take effect, unprocessed files remain
  - Results refresh automatically: groups with fewer than 2 copies disappear and reclaimable space is recalculated
  - "↩ Undo This Deletion" restores files to their original paths (missing directories are recreated; existing same-name files are skipped, never overwritten)
- **Persistent trash**: All undoable batches are listed (time / file count / size / source folder) with per-batch restore, open-trash-folder, and permanent purge. Batch records survive app restarts
- Delete / restore / purge all show progress and can be stopped; they are mutually exclusive with comparison, sync, and undo-sync tasks

### Common

- **CSV export**: UTF-8 with BOM so Chinese text opens correctly in Excel; available for comparison results and duplicate reports
- **Handy interactions**: Double-click a row to reveal the file in Explorer; copy paths / hashes; light and dark themes
- **Fault tolerance**: Unreadable or failed entries are reported without aborting the overall task
- **Fully local**: No network requests; file contents and paths never leave your machine

## Safety by Design

FileSync copies, moves, and deletes files, so preventing data loss is a first-class design goal:

| Mechanism | Description |
|---|---|
| No overwrite | Sync copies and trash restores skip existing same-name files instead of overwriting them |
| Pre-delete validation | Before deleting a duplicate, the app verifies it is inside the scan root, still exists, and still has the scanned size |
| Undoable deletion | Files go to the App trash instead of being deleted outright; batch records persist across restarts |
| Undoable sync | Copied files can be reverted, with size checks to avoid deleting files modified after copying |
| Mutually exclusive tasks | Comparison, sync, undo, scan, delete, and trash operations cannot run concurrently |
| Local processing | All hashing and file operations happen on your machine; nothing is uploaded |

> **⚠️ Important**
>
> - "Empty Trash" (清空回收站) permanently deletes files and cannot be undone.
> - Files in the trash still consume disk space; purge them once you are sure they are no longer needed.
> - Try FileSync on a test folder first, and keep a backup of important data.

## Requirements

- **OS**: Windows 10 / 11 (x64)
- **Runtime**: Microsoft Edge WebView2 Runtime (usually available with Edge on Windows 10/11; the installer can bootstrap it if missing, which requires internet)
- **UI language**: Simplified Chinese

> Windows only for now. Features such as "reveal in Explorer" depend on Windows; macOS and Linux are not supported yet.

## Download and Install

### Download

Get the latest build from either release page:

- **GitHub Releases**: <https://github.com/759Li/FileSync/releases>
- **GitCode Releases**: <https://gitcode.com/lzb759/FileSync/releases>

> If no installer has been published yet, build from source as described below.

### Install

1. Download `FileSync_<version>_x64-setup.exe`
2. Run the installer and follow the wizard
3. Launch FileSync from the Start menu or desktop

Uninstall: Settings → Apps → Installed apps → FileSync → Uninstall. The uninstaller does not remove App trash data; see the [FAQ](#faq).

### Portable build

The build output `src-tauri\target\release\filesync.exe` can be run directly, provided WebView2 Runtime is installed on the machine.

## Run from Source

### Prerequisites

| Dependency | Version / Notes |
|---|---|
| Node.js | 18+ (20 LTS recommended) |
| npm | Installed with Node.js |
| Rust | stable, MSVC toolchain |
| Visual Studio Build Tools | 2022, with the "Desktop development with C++" workload |
| WebView2 | Usually preinstalled on Windows 10/11 |

Install Rust and VS Build Tools if needed:

```powershell
winget install Rustlang.Rustup
winget install Microsoft.VisualStudio.2022.BuildTools
```

### Get the code

```powershell
# GitHub
git clone https://github.com/759Li/FileSync.git

# or GitCode
git clone https://gitcode.com/lzb759/FileSync.git

cd FileSync
```

### Start the development build

```powershell
npm install       # or npm ci for a reproducible install from package-lock.json
npm run tauri dev
```

The first run compiles the Rust dependencies and may take a while; incremental builds are much faster.

### Mirrors for users in mainland China

If dependency downloads are slow, switch to domestic mirrors:

```powershell
# npm registry
npm config set registry https://registry.npmmirror.com

# Rust crates mirror (optional; write to %USERPROFILE%\.cargo\config.toml)
# [source.crates-io]
# replace-with = "rsproxy-sparse"
# [source.rsproxy-sparse]
# registry = "https://rsproxy.cn/index/"
```

## Build and Package

```powershell
npm run tauri build
```

Outputs:

- Installer: `src-tauri\target\release\bundle\nsis\FileSync_<version>_x64-setup.exe`
- Portable executable: `src-tauri\target\release\filesync.exe`

### Offline NSIS cache (optional)

Tauri caches the NSIS toolchain under `%LOCALAPPDATA%\tauri\NSIS`. The first build downloads these files from GitHub:

- `https://github.com/tauri-apps/binary-releases/releases/download/nsis-3.11/nsis-3.11.zip` (SHA1 `EF7FF767E5CBD9EDD22ADD3A32C9B8F4500BB10D`)
- `https://github.com/tauri-apps/nsis-tauri-utils/releases/download/nsis_tauri_utils-v0.5.3/nsis_tauri_utils.dll` (SHA1 `75197FEE3C6A814FE035788D1C34EAD39349B860`)

If GitHub is unreachable, prepend a working mirror prefix (for example `https://ghproxy.net/`) to the URLs. Unzip the archive and rename the folder to `NSIS`; place the DLL under `NSIS\Plugins\x86-unicode\additional\`, and make sure required files such as `Include\Win\*.nsh` are present — otherwise the bundler will discard the cache and download again.

## Usage

### Folder Comparison & Sync

1. Launch the app and choose folders A and B in "Folder Comparison" mode
2. Click "Start Compare" (开始比对) and wait for enumeration and hashing; stop at any time
3. Review the result tabs:
   - **Duplicates**: files with identical content on both sides (grouped by hash, with both paths)
   - **Only in A** / **Only in B**: files unique to one side
   - **Errors**: entries that could not be read or traversed
4. After a complete comparison, the "Follow-up Actions" panel appears:
   - **Make Both Sides Identical**: copy A-only files to B and B-only files to A (recommended for two-way backup alignment)
   - **A-only → B** / **B-only → A**: copy unique files in one direction only
   - **Do Nothing**: close the panel; reopen it from "Sync Actions" in the stats bar
5. Sync shows progress and can be stopped; review copied / conflicted / failed counts and details when done
6. Click "Undo Sync (N)" to revert files copied during this session:
   - Files whose size no longer matches the copy-time record are kept automatically
   - Stop and resume the undo later if needed
7. Click "Export Results CSV" to save the comparison; double-click any entry to reveal it in Explorer
8. After sync or undo, the list still reflects the pre-sync snapshot; run the comparison again to verify the current state

### Duplicate Scan & Cleanup

1. Click "🔍 Duplicate Scan" at the top to switch modes
2. Choose the target folder (including all subfolders) and click "Start Scan" (开始扫描)
3. The scan runs in two stages: recursive enumeration with size prefiltering, then parallel SHA-256 hashing of same-size files
4. Results are grouped by duplicate set (largest reclaimable space first):
   - Group header: group number, per-file size × file count, reclaimable space, hash
   - Rows: full path and size; double-click to reveal the file
   - Every copy except the first is selected by default; use "Keep" or "Select All / None" to adjust
5. "Copy Selected Paths" copies all selected paths with the total size, ready to paste into a cleanup script
6. Click "🗑 Delete Selected Copies" and confirm:
   - Selected files move into the App trash after per-file validation
   - Progress is shown and can be stopped; a summary reports deleted / freed / skipped / failed counts
   - Results refresh automatically and reclaimable space is recalculated
   - "↩ Undo This Deletion" restores the batch immediately
7. Manage historical batches in the "🗑 Trash" section:
   - "↩ Restore" returns a batch to its original paths (never overwriting existing files)
   - "Open Trash Folder" shows the staged files
   - "Empty Trash" permanently deletes them — irreversible
   - Batch records persist across app restarts
8. Click "Export Report CSV" to save group details, errors, and statistics
9. Unreadable / failed files appear in the "Read Failures" tab and do not stop the scan

## FAQ

**Q: Can FileSync delete files by mistake?**

A: Before deleting a duplicate, FileSync verifies that it is inside the scan root, still exists, and still has the scanned size. Files are moved to the App trash rather than deleted outright, so they can be restored. Only "Empty Trash" is permanent.

**Q: How are duplicates detected?**

A: By SHA-256 content hash — not by file name, extension, or modification time. Same-name files with different content are not duplicates; different-name files with identical content are.

**Q: Will scanning a large folder be slow or memory-hungry?**

A: File sizes are used to prefilter candidates, and only same-size files are hashed. Hashing reads files in 4 MB chunks and runs in parallel with rayon, so memory usage stays stable. Speed is mainly limited by disk I/O.

**Q: Are macOS and Linux supported?**

A: Not yet. The current release targets Windows 10/11 x64 and the UI is in Simplified Chinese.

**Q: Can I compare an external drive or a network share?**

A: Yes. Speed depends on the device's I/O; unreadable files are collected in the error list without aborting the task.

**Q: Where does the trash live? Is it removed on uninstall?**

A: Under `%APPDATA%\com.filesync.app\filesync-trash\` (including the `batches.json` batch index). The uninstaller does not remove it; restore or empty the trash first, then delete the folder manually if you want it gone.

**Q: Does the app use the network or upload files?**

A: No. FileSync has no network functionality; all hashing and file operations are local.

**Q: Why didn't the result list change after a sync?**

A: The list is a snapshot taken before the sync. Run the comparison again to verify the actual state.

**Q: Both folders contain a file with the same name, but it is listed as "Only in A / Only in B". Why?**

A: Their contents differ. FileSync compares by SHA-256, so same-name files with different content are treated as different files.

## Tech Stack

| Layer | Technology |
|---|---|
| Frontend | React 18 + TypeScript + Vite 5 + Tailwind CSS |
| Backend | Rust (walkdir / sha2 / rayon) |
| Desktop framework | Tauri 2 (tauri-plugin-dialog) |
| Packaging | NSIS (Windows x64) |

## Project Structure

```text
FileSync/
├── src/                          # Frontend
│   ├── App.tsx                   # Main UI, mode switching, event wiring, sync/undo wiring
│   ├── types.ts                  # Types mirroring the Rust serde models
│   ├── components/
│   │   ├── FolderPicker.tsx      # Folder selection
│   │   ├── ProgressPanel.tsx     # Progress and stop control
│   │   ├── ResultsView.tsx       # Four comparison result tabs
│   │   ├── SyncPanel.tsx         # Sync action panel
│   │   ├── DupScanView.tsx       # Duplicate scan / delete / trash UI
│   │   └── ui/                   # Base UI components
│   ├── lib/                      # Formatting and style utilities
│   └── styles/                   # Global styles
├── src-tauri/                    # Backend
│   ├── src/
│   │   ├── scanner.rs            # Recursive enumeration, 4 MB chunked SHA-256, rayon
│   │   ├── compare.rs            # A∩B / A-B / B-A classification
│   │   ├── dup_scan.rs           # Duplicate scan: size prefilter + hash grouping
│   │   ├── dup_trash.rs          # Delete / restore / purge, persistent trash batches
│   │   ├── sync.rs               # Two-way harmonize / one-way copy / undo
│   │   ├── commands.rs           # Tauri command registration
│   │   ├── csv_export.rs         # UTF-8 BOM CSV export
│   │   └── state.rs              # Task state and caches
│   ├── capabilities/             # Tauri permission configuration
│   ├── icons/                    # Application icons
│   └── tauri.conf.json           # Tauri configuration
├── README.md                     # Chinese README
├── README.en.md                  # English README (this file)
├── LICENSE                       # GPL-3.0
└── package.json
```

## Contributing

Issues and pull requests are welcome:

- **Bug reports**: Include your Windows version, FileSync version, reproduction steps, and relevant logs / screenshots
- **Feature requests**: Describe the use case and expected behavior
- **Pull requests**:
  1. Fork the repository and create a feature branch
  2. Follow the existing code style and run `cargo fmt` and `npm run build` before submitting
  3. Explain the change and how you verified it

> Changes touching deletion or sync logic should explicitly discuss safety and edge cases.

If FileSync is useful to you, a Star ⭐ on GitHub / GitCode is appreciated.

## Version History

| Version | Date | Highlights |
|---|---|---|
| **0.3.0** | 2026-10-24 | One-click duplicate deletion (undoable trash), persistent batches, automatic result refresh |
| 0.2.0 | 2026-09-27 | Folder comparison and sync (harmonize / one-way copy / undo sync) |
| 0.1.0 | 2026-09-27 | Initial release: SHA-256 content comparison of two folders |

## License

Released under the **GNU General Public License v3.0 only (GPL-3.0-only)**. See [LICENSE](LICENSE).

Copyright (C) 2026 lzb759
