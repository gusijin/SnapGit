# SnapGit

**[⬇ 下载最新版 → Releases]([https://gitcode.com/gusijin1/SnapGit/releases](https://github.com/gusijin/SnapGit/tree/main/releases))**
Windows x64 / x86（32 位）`exe`、macOS Apple Silicon / Intel `dmg`

现代化的 Git 客户端，基于 Tauri 2 + Vue 3 构建。

## 功能特性

- 📁 **仓库管理** - 打开本地 Git 仓库
- 📜 **提交日志** - 查看提交历史记录
- 🌿 **分支管理** - 创建、切换、查看分支
- 📋 **文件状态** - 查看工作区文件变更，暂存文件
- ✅ **提交更改** - 提交暂存的文件更改

## 技术栈

- **前端**: Vue 3 + TypeScript + Vite
- **后端**: Rust + Tauri 2
- **Git 库**: libgit2 (git2-rs)

## 开发环境

### 前置依赖

- Node.js (>= 18)
- Rust (>= 1.70)
- Git

### 安装

```bash
# 安装前端依赖
npm install

# 安装 Rust 依赖（自动处理）
cd src-tauri && cargo build
```

### 开发模式

```bash
npm run tauri dev
```

### 构建

```bash
# 仅构建前端
npm run build

# 构建桌面应用
npm run tauri build
```

### 打包 Windows 安装包（exe）

使用 Tauri 的 NSIS 打包器生成 `.exe` 安装程序（Tauri v2 会自动下载内置 NSIS，无需手动安装）。

#### 64 位（x64，默认架构）

```bash
npm run tauri -- build --bundles nsis
```

- 产物路径：`src-tauri/target/release/bundle/nsis/SnapGit_0.1.0_x64-setup.exe`
- 也可显式指定目标架构：`npm run tauri -- build --target x86_64-pc-windows-msvc --bundles nsis`（产物移到 `src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/`）。
- `NODE_OPTIONS=` 用于清空 WorkBuddy 注入的 `genie-safe-delete` 拦截（它会劫持 vite 清空 `dist` 时的 `fs.rmSync` 走回收站，导致构建失败）；在原生终端（非 WorkBuddy 环境）运行时可省略，直接写 `npm run tauri -- build --bundles nsis` 即可。

#### 32 位（x86）

仅当目标机器是 **32 位 Windows** 时才需要此架构（64 位系统请直接用上面的 x64 包，安装 32 位程序没有意义）。

```bash
# 1. 添加 32 位编译目标（已添加过可跳过）
rustup target add i686-pc-windows-msvc

# 2. 按 32 位目标构建 NSIS 安装包
npm run tauri -- build --target i686-pc-windows-msvc --bundles nsis
```

- 产物路径：`src-tauri/target/i686-pc-windows-msvc/release/bundle/nsis/SnapGit_0.1.0_x86-setup.exe`
- **产物目录注意**：指定 `--target` 后构建输出不在默认的 `target/release`，而是 `target/<target-triple>/release`，路径里多一层 `i686-pc-windows-msvc`。
- **前置依赖**：Visual Studio Build Tools 需包含 x86 的 MSVC 编译组件（安装「使用 C++ 的桌面开发」工作负载时默认已带 x86/x64 工具集，通常无需额外操作）。
- 主程序与 NSIS 安装包均为 32 位，可正常安装运行于 32 位 Windows；WebView2 Runtime 安装器会自动拉取匹配架构的版本。
- 如需同时产出 x64 与 x86 两个安装包，分别执行上面的 x64 / x86 命令即可（产物分属不同 target 目录，互不覆盖）。

### 打包 macOS 安装包（dmg）

⚠️ **必须在 macOS 环境下构建**，Apple 不允许从 Windows/Linux 交叉编译 macOS 应用包。请在 MacBook（或 macOS CI）上执行以下命令：

```bash
# 生成 .dmg 磁盘映像安装包（最常用）
npm run tauri -- build --bundles dmg

# 同时生成 .app 应用包 + .dmg
npm run tauri -- build --bundles app,dmg
```

- 产物路径：`src-tauri/target/release/bundle/dmg/SnapGit_0.1.0_aarch64.dmg`（Apple Silicon）或 `..._x64.dmg`（Intel）。
- `npm run tauri build`（不带 `--bundles`）会按 `bundle.targets: "all"` 构建当前平台全部可用包，在 Mac 上等价于 `app,dmg,updater`。
- 图标已配置 `icons/icon.icns`，无需额外准备。
- **签名与公证（分发到他人 Mac 时必需）**：默认产物为「未签名」应用，在本机可运行但其他 Mac 会因 Gatekeeper 拦截。如需对外分发，需在 `tauri.conf.json` 的 `bundle.macOS` 中配置 Apple 开发者证书（`signingIdentity`）并执行公证（`notarize`）。仅自用/内部分发可忽略。
- `NODE_OPTIONS=` 前缀作用同 Windows 打包说明，非 WorkBuddy 环境可省略。

> 跨平台构建建议：若主力开发机是 Windows，可直接用下面的 GitHub Actions 工作流把 Windows + macOS 四个包一次性打完，无需在 MacBook 上手动操作。

### 使用 GitHub Actions 自动打包

已内置工作流 `.github/workflows/release.yml`，一次运行产出 4 个安装包：Windows x64、Windows x86（32 位）、macOS Apple Silicon、macOS Intel。

**触发方式**

- **手动**：GitHub 仓库页面 → `Actions` → `Release` → `Run workflow`（可勾选「把构建产物提交回仓库的 releases/ 目录」）。
- **推 tag 自动触发**：

```bash
git tag v0.1.0
git push origin v0.1.0
```

推 tag 时除构建外，还会自动发布到 GitHub Release 页面，`releases/` 下的文件全部作为附件上传。

**产物位置**

所有平台的安装包在最后一个 `collect` job 里统一汇总到仓库根目录的 `releases/` 目录（用 `download-artifact` 的 `merge-multiple` 展平，不保留平台子目录）：

```
releases/
├── SnapGit_0.1.0_x64-setup.exe       # Windows 64 位
├── SnapGit_0.1.0_x86-setup.exe       # Windows 32 位
├── SnapGit_0.1.0_aarch64.dmg         # macOS Apple Silicon
└── SnapGit_0.1.0_x64.dmg             # macOS Intel
```

**获取产物**

- 在 Actions 运行页底部下载名为 `SnapGit-Release` 的构建产物，解压后即为完整的 `releases/` 目录。
- 手动触发并勾选回写选项时，产物会由 `github-actions[bot]` 自动 commit 到当前分支的 `releases/` 目录（不动其他文件）；若 `releases/` 无变化则跳过提交。

**注意事项**

- 手动触发时若不想让二进制进仓库，取消勾选回写选项即可，产物仍可在 Actions 页面下载。
- 推 tag 触发时 checkout 处于 detached HEAD，因此**只发布 GitHub Release、不回写分支**。
- 工作流使用 `tauri-apps/tauri-action`，各平台 job 独立运行（`fail-fast: false`），单个平台失败不影响其他平台产物。
- macOS 产物默认未签名，外发给其他 Mac 需配置签名与公证（见上文「打包 macOS 安装包（dmg）」）。

## 项目结构

```
SnapGit/
├── .github/
│   └── workflows/
│       └── release.yml       # GitHub Actions 自动打包工作流
├── releases/                 # 打包产物输出目录（CI 统一汇总到此）
├── src/                      # 前端源码
│   ├── api/                  # API 接口
│   │   └── git.ts            # Git 相关 API 调用
│   ├── router/               # 路由配置
│   │   └── index.ts          # Vue Router 配置
│   ├── types/                # TypeScript 类型定义
│   │   └── index.ts          # 共享类型
│   ├── views/                # 页面组件
│   │   ├── Home.vue          # 首页
│   │   └── Repository.vue    # 仓库详情页
│   ├── App.vue               # 根组件
│   ├── main.ts               # 入口文件
│   └── style.css             # 全局样式
├── src-tauri/                # Tauri 后端源码
│   ├── src/
│   │   └── main.rs           # Rust 主程序，包含 Git 操作逻辑
│   ├── Cargo.toml            # Rust 依赖配置
│   └── icons/                # 应用图标
├── index.html                # HTML 模板
├── package.json              # 前端依赖配置
├── vite.config.ts            # Vite 配置
└── tsconfig.json             # TypeScript 配置
```

## Git 操作 API

后端提供以下 Git 操作命令：

| 命令 | 功能 |
|------|------|
| `open_repository` | 打开仓库，获取仓库信息 |
| `get_commits` | 获取提交历史 |
| `get_branches` | 获取分支列表 |
| `get_file_status` | 获取文件状态 |
| `stage_file` | 暂存文件 |
| `commit` | 提交更改 |
| `checkout_branch` | 切换分支 |
| `create_branch` | 创建分支 |
| `get_current_branch` | 获取当前分支 |
| `open_folder_dialog` | 打开文件夹选择对话框 |

## 许可证

MIT
