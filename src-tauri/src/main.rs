#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{LazyLock, Mutex, RwLock};
use std::time::{Duration, Instant};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use tauri::command;

use git2::{BranchType, DiffOptions, Repository, Status, StatusOptions};
#[cfg(target_os = "macos")]
use tauri::menu::{
    CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu,
};
#[allow(unused_imports)]
use tauri::{AppHandle, Emitter, Manager, Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder, Window};

type Result<T> = std::result::Result<T, String>;

// 读写锁：写命令（commit/checkout/push/stage 等）独占（与读、写都互斥），只读命令
// （get_commits/get_branches/get_file_status/stash_list 等）可并发共享。既保留原保护语义
// —— 防止并发 checkout（写）与其他读命令交错损坏 .git 对象库、进程崩溃（闪退），
// 又让 loadRepoData 的多个只读命令真正并行，不再因全局串行而排队（切换仓库卡顿的根因）。
static GIT_LOCK: RwLock<()> = RwLock::new(());

/// 正在执行中的 Git 操作计数：guard 获取时 +1，Drop 时 -1。
/// 关闭主窗口时据此判断「是否有 Git 进程/操作仍在跑」，忙则弹确认框。
static ACTIVE_GIT_OPS: AtomicUsize = AtomicUsize::new(0);

/// 只读 guard：持有 GIT_LOCK 读锁并计入 ACTIVE_GIT_OPS。
struct GitReadGuard(#[allow(dead_code)] std::sync::RwLockReadGuard<'static, ()>);
impl Drop for GitReadGuard {
    fn drop(&mut self) {
        ACTIVE_GIT_OPS.fetch_sub(1, Ordering::SeqCst);
    }
}
fn git_read_guard() -> GitReadGuard {
    let g = GIT_LOCK.read().unwrap_or_else(|e| e.into_inner());
    ACTIVE_GIT_OPS.fetch_add(1, Ordering::SeqCst);
    GitReadGuard(g)
}

/// 写 guard：持有 GIT_LOCK 写锁并计入 ACTIVE_GIT_OPS。
struct GitWriteGuard(#[allow(dead_code)] std::sync::RwLockWriteGuard<'static, ()>);
impl Drop for GitWriteGuard {
    fn drop(&mut self) {
        ACTIVE_GIT_OPS.fetch_sub(1, Ordering::SeqCst);
    }
}
fn git_write_guard() -> GitWriteGuard {
    let g = GIT_LOCK.write().unwrap_or_else(|e| e.into_inner());
    ACTIVE_GIT_OPS.fetch_add(1, Ordering::SeqCst);
    GitWriteGuard(g)
}

// 文件状态缓存：自动刷新高频调用 get_file_status 时命中，避免每次都扫 .git。
// TTL 短（800ms）以控制 stale；loadRepoData 等主动刷新走 cached=false 绕过，
// 保证切库 / 写操作后即时准确。仅缓存 get_file_status（autoRefresh 高频路径）。
static STATUS_CACHE: LazyLock<Mutex<HashMap<String, (Instant, Vec<FileStatus>)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
const STATUS_CACHE_TTL: Duration = Duration::from_millis(800);

/// 仓库实例缓存：避免每次命令都 `Repository::open` 重扫 .git（大仓库每次几十 ms）。
/// 通过 remove/insert 转移所有权，保证同一时刻只有一个任务持有某仓库实例
/// （git2::Repository 不是 Sync，但可在任务间 Send 移动，无需共享借用）。
static REPO_CACHE: LazyLock<Mutex<HashMap<String, git2::Repository>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
const REPO_CACHE_LIMIT: usize = 8;

/// 取仓库实例：优先复用缓存（remove 出所有权），否则新建。错误已 map 成 String 以便 `?` 传播。
fn open_repo(repo_path: &str) -> Result<git2::Repository> {
    if let Some(repo) = REPO_CACHE.lock().unwrap().remove(repo_path) {
        return Ok(repo);
    }
    git2::Repository::open(repo_path).map_err(|e| e.to_string())
}

/// 查询当前是否有 Git 操作正在执行（供关闭窗口前的确认弹窗使用）。
#[command]
fn is_git_busy() -> bool {
    ACTIVE_GIT_OPS.load(Ordering::SeqCst) > 0
}

/// 归还仓库实例到缓存（调用方不再持有）。超过上限则整体清空，避免多仓库无限累积。
fn return_repo(repo_path: &str, repo: git2::Repository) {
    let mut cache = REPO_CACHE.lock().unwrap();
    if cache.len() >= REPO_CACHE_LIMIT {
        cache.clear();
    }
    cache.insert(repo_path.to_string(), repo);
}

/// 构造 git CLI 子进程命令。
/// Windows 上设置 CREATE_NO_WINDOW (0x08000000)，避免主程序（GUI 子系统、无控制台）
/// spawn 控制台子进程 git.exe 时被分配一个控制台窗口并闪烁。
/// 输出/错误仍可被父进程正常捕获，行为等价于后台运行。
fn git_command() -> std::process::Command {
    let mut cmd = std::process::Command::new("git");
    #[cfg(windows)]
    cmd.creation_flags(0x08000000);
    cmd
}

// 编辑窗口参数仓库：key = 窗口 label，value = { repoPath, filePath, mode }。
// 独立 webview 的 initialization_script 注入不可靠，改为前端 mount 后主动来取。
static EDIT_ARGS_MAP: std::sync::LazyLock<Mutex<std::collections::HashMap<String, serde_json::Value>>> =
    std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

#[derive(Debug, serde::Serialize)]
struct Commit {
    id: String,
    message: String,
    author: String,
    date: String,
}

#[derive(Debug, serde::Serialize)]
struct Branch {
    name: String,
    is_current: bool,
    is_remote: bool,
    ahead: usize,
    behind: usize,
}

#[derive(Debug, serde::Serialize)]
struct UpstreamInfo {
    remote: String,
    remote_branch: String,
}

/// 检测远程分支是否可被当前用户删除。
/// 通过两步判断：
/// 1. 协议快速检查：`git://` 协议只读，直接判不可写。
/// 2. `git push --dry-run <remote> :<remote_branch>` 试运行：捕获服务端拒绝原因，
///    若错误包含"protected / denied / permission / forbidden"等关键词则不可删；
///    其他错误（如认证失败、网络错误）视为"未知"，让用户实际执行时再试。
#[derive(Debug, serde::Serialize)]
struct RemoteDeleteCheck {
    can_delete: bool,
    /// 当 can_delete=false 时，填写中文简短原因，前端用作 tooltip。
    reason: Option<String>,
}

#[derive(Debug, serde::Serialize, Clone)]
struct FileStatus {
    path: String,
    status: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
struct ScannedProject {
    path: String,
    name: String,
    vcs_type: String,
}

#[derive(Debug, serde::Serialize)]
struct FileTreeNode {
    name: String,
    path: String,
    is_dir: bool,
    children: Vec<FileTreeNode>,
}

#[derive(Debug, serde::Serialize, Clone)]
struct DiffSegment {
    text: String,
    changed: bool,
}

#[derive(Debug, serde::Serialize, Clone)]
struct DiffLine {
    line_type: String,
    content: String,
    new_content: String,
    old_line: Option<usize>,
    new_line: Option<usize>,
    old_segments: Vec<DiffSegment>,
    new_segments: Vec<DiffSegment>,
}

#[derive(Debug, serde::Serialize)]
struct FileDiff {
    old_content: Vec<String>,
    new_content: Vec<String>,
    lines: Vec<DiffLine>,
    /// 二进制文件（exe / 图片 / 压缩包等）：不做逐行 diff，前端展示「无法预览」提示
    is_binary: bool,
    /// 超大文本文件：行数超过 MAX_DIFF_LINES，跳过 O(m*n) 的 LCS 计算（否则会卡死/OOM），
    /// 前端降级为「仅展示可编辑、无差异高亮」并提示用户
    is_oversized: bool,
}

#[derive(Debug, serde::Serialize, Clone)]
struct ConflictBlock {
    start_line: usize,
    separator_line: usize,
    end_line: usize,
    ours: Vec<String>,
    theirs: Vec<String>,
    base: Option<Vec<String>>,
    marker_ours: String,
    marker_theirs: String,
}

#[derive(Debug, serde::Serialize)]
struct ConflictFile {
    path: String,
    ours_content: Vec<String>,
    theirs_content: Vec<String>,
    base_content: Option<Vec<String>>,
    working_content: Vec<String>,
    blocks: Vec<ConflictBlock>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
struct RepositoryInfo {
    path: String,
    name: String,
    current_branch: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct RecentRepositories {
    repos: Vec<RepositoryInfo>,
}

fn get_recent_file_path(app: &AppHandle) -> PathBuf {
    let app_data_dir = app.path().app_data_dir().expect("Failed to get app data directory");
    fs::create_dir_all(&app_data_dir).expect("Failed to create app data directory");
    app_data_dir.join("recent_repos.json")
}

#[command]
fn load_recent_repositories(app: AppHandle) -> Result<Vec<RepositoryInfo>> {
    let _guard = git_read_guard();
    let file_path = get_recent_file_path(&app);
    
    if !file_path.exists() {
        return Ok(Vec::new());
    }
    
    let file = File::open(&file_path).map_err(|e| e.to_string())?;
    let reader = BufReader::new(file);
    let recent: RecentRepositories = serde_json::from_reader(reader).map_err(|e| e.to_string())?;
    
    Ok(recent.repos)
}

#[command]
fn save_recent_repository(app: AppHandle, repo_info: RepositoryInfo) -> Result<()> {
    let _guard = git_read_guard();
    let file_path = get_recent_file_path(&app);
    
    let mut repos = if file_path.exists() {
        let file = File::open(&file_path).map_err(|e| e.to_string())?;
        let reader = BufReader::new(file);
        let recent: RecentRepositories = serde_json::from_reader(reader).map_err(|e| e.to_string())?;
        recent.repos
    } else {
        Vec::new()
    };
    
    repos.retain(|r| r.path != repo_info.path);
    repos.insert(0, repo_info);
    if repos.len() > 10 {
        repos.truncate(10);
    }
    
    let file = File::create(&file_path).map_err(|e| e.to_string())?;
    let writer = BufWriter::new(file);
    serde_json::to_writer(writer, &RecentRepositories { repos }).map_err(|e| e.to_string())?;
    
    Ok(())
}

#[command]
fn remove_recent_repository(app: AppHandle, path: String) -> Result<()> {
    let _guard = git_read_guard();
    let file_path = get_recent_file_path(&app);
    
    if !file_path.exists() {
        return Ok(());
    }
    
    let mut repos = {
        let file = File::open(&file_path).map_err(|e| e.to_string())?;
        let reader = BufReader::new(file);
        let recent: RecentRepositories = serde_json::from_reader(reader).map_err(|e| e.to_string())?;
        recent.repos
    };
    
    repos.retain(|r| r.path != path);
    
    let file = File::create(&file_path).map_err(|e| e.to_string())?;
    let writer = BufWriter::new(file);
    serde_json::to_writer(writer, &RecentRepositories { repos }).map_err(|e| e.to_string())?;
    
    Ok(())
}

#[command]
fn open_repository(path: String) -> Result<RepositoryInfo> {
    let _guard = git_read_guard();
    let repo = Repository::open(&path).map_err(|e| {
        // 给"目录不是有效 Git 仓库"这种错误加特定前缀，
        // 前端 catch 时据此判断是否弹"是否初始化为新仓库"对话框
        let msg = e.to_string();
        if matches!(e.code(), git2::ErrorCode::NotFound) {
            format!("NOT_A_REPO:{}", msg)
        } else {
            msg
        }
    })?;

    let repo_name = PathBuf::from(&path)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let head = repo.head().map_err(|e| e.to_string())?;
    let current_branch = head.shorthand().unwrap_or("detached").to_string();

    Ok(RepositoryInfo {
        path,
        name: repo_name,
        current_branch,
    })
}

/// 把指定目录初始化为新 Git 仓库。
/// 默认主分支为 `main`（不是旧式 `master`），符合现代约定。
/// 用户在"打开不是仓库的目录"时，确认 Initialize 后调用此命令初始化，
/// 然后再次调用 `open_repository` 完成加载。
#[command]
fn init_repository(repo_path: String) -> Result<RepositoryInfo> {
    let _guard = git_read_guard();

    let path_buf = PathBuf::from(&repo_path);
    if !path_buf.is_dir() {
        return Err(format!("路径 {} 不是有效目录", repo_path));
    }

    let mut opts = git2::RepositoryInitOptions::new();
    opts.initial_head("main");
    opts.mkpath(true);
    opts.mkdir(true);
    Repository::init_opts(&repo_path, &opts)
        .map_err(|e| format!("初始化仓库失败: {}", e))?;

    // 复用 open_repository 的结果提取（仓库名、HEAD 分支）
    let repo = Repository::open(&repo_path).map_err(|e| e.to_string())?;
    let repo_name = path_buf
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let current_branch = repo
        .head()
        .ok()
        .and_then(|h| h.shorthand().map(|s| s.to_string()))
        .unwrap_or_else(|| "main".to_string());

    Ok(RepositoryInfo {
        path: repo_path,
        name: repo_name,
        current_branch,
    })
}

#[command]
async fn get_commits(repo_path: String, limit: usize, skip: Option<usize>) -> Result<Vec<Commit>> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_read_guard();
        let repo = Repository::open(&repo_path).map_err(|e| e.to_string())?;

        // 空仓库（unborn HEAD，例如刚 init 还没首次 commit）没有可遍历的历史。
        // 直接返回空数组，让前端分支/日志面板正常显示空状态，
        // 而非把整个 `Promise.all` 拉爆导致面板残留上一个仓库的数据。
        // （参考: libgit2 在 unborn HEAD 上 `revwalk.push_head()` 返回 `UnbornBranch` 错误）
        if repo.head().is_err() {
            return Ok(Vec::new());
        }

        let mut revwalk = repo.revwalk().map_err(|e| e.to_string())?;
        revwalk.push_head().map_err(|e| e.to_string())?;

        // 分页：skip 跳过最新的 N 条（LogView 滚动到底时加载更早的历史）
        let skip_n = skip.unwrap_or(0);
        let mut commits = Vec::new();
        for id in revwalk.skip(skip_n).take(limit) {
            let id = id.map_err(|e| e.to_string())?;
            let commit = repo.find_commit(id).map_err(|e| e.to_string())?;

            commits.push(Commit {
                id: id.to_string(),
                message: commit.message().unwrap_or("").trim().to_string(),
                author: commit.author().name().unwrap_or("").to_string(),
                date: commit.time().seconds().to_string(),
            });
        }

        Ok(commits)
    })
    .await
    .map_err(|e| format!("加载提交记录失败: {}", e))?
}

// 本地分支未配置上游时，对比第一个远程的 HEAD（默认分支）估算"本地需要上传"的提交数
// 只取 ahead 分量（behind 置 0），避免未配置上游的分支显示误导性的"落后"数
fn remote_head_ahead(repo: &Repository, local_oid: git2::Oid) -> (usize, usize) {
    let Ok(remotes) = repo.remotes() else { return (0, 0) };
    for i in 0..remotes.len() {
        let Some(name) = remotes.get(i) else { continue };
        let head_ref = format!("refs/remotes/{}/HEAD", name);
        if let Ok(obj) = repo.revparse_single(&head_ref) {
            if let Ok(commit) = obj.peel_to_commit() {
                return repo
                    .graph_ahead_behind(local_oid, commit.id())
                    .unwrap_or((0, 0));
            }
        }
    }
    (0, 0)
}

#[command]
async fn get_branches(repo_path: String, full: Option<bool>) -> Result<Vec<Branch>> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_read_guard();
        let repo = Repository::open(&repo_path).map_err(|e| e.to_string())?;

        // 空仓库（unborn HEAD）：HEAD 文件指向 `refs/heads/<name>` 但没有 commit，
        // `repo.head()` 会返回 Err。这里 fallback 到默认分支名（"main"），
        // 分支列表天然为 Vec::new()，与「还没有任何分支」的事实一致。
        // 前端分支面板进入空状态即可，绝不让整段挂在 `head.shorthand()?` 上。
        let current_branch = repo
            .head()
            .ok()
            .and_then(|h| h.shorthand().map(|s| s.to_string()))
            .unwrap_or_else(|| "main".to_string());

        let mut branches = Vec::new();

        for branch in repo.branches(None).map_err(|e| e.to_string())? {
            let (branch, branch_type) = branch.map_err(|e| e.to_string())?;
            let name = branch.name().map_err(|e| e.to_string())?
                .map(|s| s.to_string())
                .unwrap_or_else(|| "unknown".to_string());

            let is_current = name == current_branch;
            let is_remote = branch_type == BranchType::Remote;

            // 过滤掉远程的 HEAD 符号引用（如 origin/HEAD），它不是真实分支
            if is_remote && name.ends_with("/HEAD") {
                continue;
            }

            // 本地分支：计算相对于上游分支的 ahead/behind 提交数。
            // C2 优化：切库（full=false）时只为当前分支计算，其余置 0，
            // 避免遍历所有分支各做一次图遍历导致切换仓库卡顿；
            // 分支面板需要完整数据时以 full=true 重新拉取补全。
            let (ahead, behind) = if !is_remote {
                if full.unwrap_or(false) || is_current {
                    match branch.upstream() {
                        Ok(upstream) => {
                            let local_oid = branch.get().target();
                            let upstream_oid = upstream.get().target();
                            match (local_oid, upstream_oid) {
                                (Some(local), Some(upstream)) => {
                                    repo.graph_ahead_behind(local, upstream)
                                        .unwrap_or((0, 0))
                                }
                                _ => (0, 0),
                            }
                        }
                        Err(_) => {
                            // 无上游（如新建后未推送的分支）：对比远程 HEAD 估算"本地需要上传"的提交数
                            match branch.get().target() {
                                Some(local) => {
                                    let (ahead, _) = remote_head_ahead(&repo, local);
                                    (ahead, 0)
                                }
                                None => (0, 0),
                            }
                        }
                    }
                } else {
                    (0, 0)
                }
            } else {
                (0, 0)
            };

            branches.push(Branch {
                name,
                is_current,
                is_remote,
                ahead,
                behind,
            });
        }

        Ok(branches)
    })
    .await
    .map_err(|e| format!("加载分支失败: {}", e))?
}

#[command]
async fn get_remotes(repo_path: String) -> Result<Vec<String>> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_read_guard();
        let repo = Repository::open(&repo_path).map_err(|e| e.to_string())?;
        let remotes = repo.remotes().map_err(|e| e.to_string())?;
        let mut result = Vec::new();
        for i in 0..remotes.len() {
            if let Some(name) = remotes.get(i) {
                result.push(name.to_string());
            }
        }
        Ok(result)
    })
    .await
    .map_err(|e| format!("获取远程列表失败: {}", e))?
}

#[command]
async fn get_upstream(repo_path: String, branch_name: String) -> Result<Option<UpstreamInfo>> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_read_guard();
        let repo = Repository::open(&repo_path).map_err(|e| e.to_string())?;
        let branch = repo.find_branch(&branch_name, BranchType::Local)
            .map_err(|e| e.to_string())?;

        let result = match branch.upstream() {
            Ok(upstream) => {
                let name = upstream.name().map_err(|e| e.to_string())?
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                if let Some((remote, remote_branch)) = name.split_once('/') {
                    Ok(Some(UpstreamInfo {
                        remote: remote.to_string(),
                        remote_branch: remote_branch.to_string(),
                    }))
                } else {
                    Ok(None)
                }
            }
            Err(_) => Ok(None),
        };
        result
    })
    .await
    .map_err(|e| format!("获取上游分支失败: {}", e))?
}

// 判定是否为合并冲突的未合并状态（git status --porcelain 的 7 种组合）
fn is_conflict_status(x: char, y: char) -> bool {
    matches!(
        (x, y),
        ('U', 'U') | ('A', 'A') | ('D', 'D') | ('A', 'U') | ('U', 'A') | ('D', 'U') | ('U', 'D')
    )
}

// 将 libgit2 的 Status 位掩码映射到与 `git status --porcelain` 等价的 (x, y) 字符，
// 以便复用原有状态字符串映射，保证 UI 展示与旧 CLI 实现完全一致。
fn status_xy(status: Status) -> (char, char) {
    if status.contains(Status::CONFLICTED) {
        return ('U', 'U');
    }
    let mut x = ' ';
    let mut y = ' ';
    if status.contains(Status::INDEX_NEW) {
        x = 'A';
    } else if status.contains(Status::INDEX_MODIFIED) {
        x = 'M';
    } else if status.contains(Status::INDEX_DELETED) {
        x = 'D';
    } else if status.contains(Status::INDEX_RENAMED) {
        x = 'R';
    } else if status.contains(Status::INDEX_TYPECHANGE) {
        x = 'T';
    }
    if status.contains(Status::WT_NEW) {
        x = '?';
        y = '?';
    } else if status.contains(Status::WT_MODIFIED) {
        y = 'M';
    } else if status.contains(Status::WT_DELETED) {
        y = 'D';
    } else if status.contains(Status::WT_RENAMED) {
        y = 'R';
    } else if status.contains(Status::WT_TYPECHANGE) {
        y = 'T';
    }
    (x, y)
}

// 与旧 CLI 实现完全一致的 status 字符串映射（XY → 语义）。
fn map_status(x: char, y: char) -> Option<&'static str> {
    if x == '?' && y == '?' {
        Some("untracked")
    } else if is_conflict_status(x, y) {
        Some("conflict")
    } else if x != ' ' {
        match x {
            'A' | 'C' => Some("new"),
            'M' => Some("modified"),
            'D' => Some("deleted"),
            'R' => Some("renamed"),
            'T' => Some("typechange"),
            _ => Some("unknown"),
        }
    } else if y != ' ' {
        match y {
            'A' | 'C' => Some("new"),
            'M' => Some("modified"),
            'D' => Some("deleted"),
            'R' => Some("renamed"),
            'T' => Some("typechange"),
            _ => Some("unknown"),
        }
    } else {
        None
    }
}

#[command]
async fn get_file_status(repo_path: String, cached: Option<bool>) -> Result<Vec<FileStatus>> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_read_guard();

        // C4: 自动刷新高频调用时命中缓存，避免每次都扫 .git
        let use_cache = cached.unwrap_or(true);
        if use_cache {
            let cache = STATUS_CACHE.lock().unwrap();
            if let Some((t, data)) = cache.get(&repo_path) {
                if t.elapsed() < STATUS_CACHE_TTL {
                    return Ok(data.clone());
                }
            }
        }

        let repo = open_repo(&repo_path)?;
        let mut opts = StatusOptions::new();
        opts.include_untracked(true);
        // 默认行为下 libgit2 会把「未跟踪目录」折叠成一条目录项（如 `releases/`），
        // 目录内新增的所有文件都不会出现在变更列表里，等价于 `git status`（不带 -uall）。
        // 变更文件面板要求所有新增文件逐个可见（不限制类型、不折叠目录），
        // 因此递归展开未跟踪目录，等价于 `git status --porcelain -uall`。
        opts.recurse_untracked_dirs(true);
        let statuses = repo.statuses(Some(&mut opts)).map_err(|e| e.to_string())?;

        let mut file_statuses = Vec::new();
        for s in statuses.iter() {
            let (x, y) = status_xy(s.status());
            let path = match s.path() {
                Some(p) => p.to_string(),
                None => continue,
            };
            if path.is_empty() {
                continue;
            }
            // libgit2 的 path() 对重命名已返回新名称，等价于原 porcelain "old -> new" 取新名
            if let Some(status_str) = map_status(x, y) {
                file_statuses.push(FileStatus {
                    path,
                    status: status_str.to_string(),
                });
            }
        }

        if use_cache {
            STATUS_CACHE
                .lock()
                .unwrap()
                .insert(repo_path.clone(), (Instant::now(), file_statuses.clone()));
        }
        // 释放对 repo 的借用（statuses 仍持有），否则无法 move repo 归还缓存
        drop(statuses);
        // 归还仓库实例，供后续命令复用（避免重复 Repository::open 扫 .git）
        return_repo(&repo_path, repo);
        Ok(file_statuses)
    })
    .await
    .map_err(|e| format!("加载文件状态失败: {}", e))?
}

#[command]
async fn stage_file(repo_path: String, file_path: String) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_write_guard();
        // 使用 git add -A 一次性处理新增、修改、删除等所有变更
        let output = git_command()
            .arg("-C")
            .arg(&repo_path)
            .arg("add")
            .arg("-A")
            .arg("--")
            .arg(&file_path)
            .output()
            .map_err(|e| format!("无法执行 git add 命令: {}", e))?;

        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let msg = format!("{}{}", stderr, stdout);
            Err(if msg.trim().is_empty() { "暂存失败".to_string() } else { msg })
        }
    })
    .await
    .map_err(|e| format!("暂存文件失败: {}", e))?
}

#[command]
async fn discard_file(repo_path: String, file_path: String) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_write_guard();
        // 先用 git ls-files 判断文件是否被追踪
        let ls_output = git_command()
            .arg("-C")
            .arg(&repo_path)
            .arg("ls-files")
            .arg("--error-unmatch")
            .arg("--")
            .arg(&file_path)
            .output()
            .map_err(|e| format!("无法执行 git ls-files 命令: {}", e))?;

        if ls_output.status.success() {
            // 追踪文件：用 git checkout 恢复
            let output = git_command()
                .arg("-C")
                .arg(&repo_path)
                .arg("checkout")
                .arg("--")
                .arg(&file_path)
                .output()
                .map_err(|e| format!("无法执行 git checkout 命令: {}", e))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);
                let msg = format!("{}{}", stderr, stdout);
                return Err(if msg.trim().is_empty() { "丢弃修改失败".to_string() } else { msg });
            }
        } else {
            // 未追踪文件：直接删除
            let full_path = std::path::Path::new(&repo_path).join(&file_path);
            if full_path.exists() {
                if full_path.is_dir() {
                    std::fs::remove_dir_all(&full_path)
                        .map_err(|e| format!("无法删除目录: {}", e))?;
                } else {
                    std::fs::remove_file(&full_path)
                        .map_err(|e| format!("无法删除文件: {}", e))?;
                }
            }
        }
        Ok(())
    })
    .await
    .map_err(|e| format!("丢弃修改失败: {}", e))?
}

#[command]
async fn commit(repo_path: String, message: String) -> Result<String> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_write_guard();
        // 使用 git 命令行提交，自动处理签名、钩子等配置
        let output = git_command()
            .arg("-C")
            .arg(&repo_path)
            .arg("commit")
            .arg("-m")
            .arg(&message)
            .output()
            .map_err(|e| format!("无法执行 git commit 命令: {}", e))?;

        if output.status.success() {
            // 从输出中提取 commit id
            let stdout = String::from_utf8_lossy(&output.stdout);
            // 典型输出: "[branchname abc1234] message"
            let first_line = stdout.lines().next().unwrap_or("");
            let commit_id = first_line
                .split_whitespace()
                .last()
                .unwrap_or("unknown")
                .to_string();
            Ok(commit_id)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let msg = format!("{}{}", stderr, stdout);
            Err(if msg.trim().is_empty() { "提交失败".to_string() } else { msg })
        }
    })
    .await
    .map_err(|e| format!("提交失败: {}", e))?
}

#[command]
async fn push(repo_path: String, remote: String, local_branch: String, remote_branch: String) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_write_guard();
        // 使用系统 git 命令行推送，自动复用 credential.helper / SSH agent 等认证配置
        let refspec = if remote_branch.is_empty() {
            local_branch.clone()
        } else {
            format!("{}:{}", local_branch, remote_branch)
        };

        let output = git_command()
            .arg("-C")
            .arg(&repo_path)
            .arg("push")
            // -u：推送成功后设置上游跟踪（branch.<name>.remote/merge），
            // 使分支面板的 ahead/behind 能基于正确的上游刷新，推送新分支后徽标归零
            .arg("-u")
            .arg("--no-progress")
            .arg(&remote)
            .arg(&refspec)
            .env("GIT_TERMINAL_PROMPT", "0")  // 禁用交互式凭证提示，防止无终端时挂起
            .output()
            .map_err(|e| format!("无法执行 git 命令: {}", e))?;

        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let mut msg = format!("{}{}", stderr, stdout);
            if msg.trim().is_empty() {
                msg = "推送失败（未知错误）".to_string();
            }
            // 常见认证失败错误，提供更友好的提示
            if msg.contains("Authentication failed") || msg.contains("could not read Username") || msg.contains("terminal prompts disabled") {
                msg = format!("推送失败：远程仓库认证失败\n\n请确保已配置认证方式（SSH 密钥或 credential helper）。\n\n原始错误:\n{}", msg);
            }
            Err(msg)
        }
    })
    .await
    .map_err(|e| format!("推送失败: {}", e))?
}

#[command]
async fn get_remote_url(repo_path: String, remote_name: String) -> Result<String> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_read_guard();
        let repo = Repository::open(&repo_path).map_err(|e| e.to_string())?;
        let remote = repo.find_remote(&remote_name).map_err(|e| e.to_string())?;
        let url = remote.url().ok_or("无法获取远程 URL")?;
        Ok(url.to_string())
    })
    .await
    .map_err(|e| format!("获取远程 URL 失败: {}", e))?
}

#[command]
async fn get_repo_config(repo_path: String) -> Result<serde_json::Value> {
    let result = tokio::task::spawn_blocking(move || {
        let _guard = git_read_guard();

        let mut map = serde_json::Map::new();

        // 1. remotes：git remote -v
        let mut remotes: Vec<serde_json::Value> = Vec::new();
        if let Ok(output) = git_command()
            .arg("-C").arg(&repo_path)
            .arg("remote").arg("-v")
            .output()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // remote -v 格式：<name>\t<url> (<type>)
            let mut seen = std::collections::HashMap::new();
            for line in stdout.lines() {
                let line = line.trim();
                if line.is_empty() { continue; }
                if let Some((name, rest)) = line.split_once('\t') {
                    let url_part = rest.trim();
                    let url = if let Some(idx) = url_part.rfind('(') {
                        url_part[..idx].trim().to_string()
                    } else {
                        url_part.to_string()
                    };
                    let entry: &mut (String, String) = seen.entry(name.to_string())
                        .or_insert_with(|| (String::new(), String::new()));
                    // 简单判断：先出现 fetch 再出现 push（git remote -v 默认顺序是 fetch, push）
                    if entry.0.is_empty() {
                        entry.0 = url.clone();
                    } else if entry.1.is_empty() {
                        entry.1 = url.clone();
                    }
                }
            }
            for (name, (fetch_url, push_url)) in &seen {
                let mut obj = serde_json::Map::new();
                obj.insert("name".into(), serde_json::Value::String(name.clone()));
                obj.insert("fetch_url".into(), serde_json::Value::String(fetch_url.clone()));
                obj.insert("push_url".into(), serde_json::Value::String(push_url.clone()));
                remotes.push(serde_json::Value::Object(obj));
            }
        }
        remotes.sort_by(|a, b| {
            a["name"].as_str().cmp(&b["name"].as_str())
        });
        map.insert("remotes".into(), serde_json::Value::Array(remotes));

        // 辅助：读取某条 git config（local 优先 → global → system）
        let read_config = |repo_path: &str, key: &str| -> Option<String> {
            for scope in ["--local", "--global", "--system"] {
                let mut cmd = git_command();
                cmd.arg("-C").arg(repo_path)
                    .arg("config").arg(scope).arg("--get").arg(key);
                if let Ok(output) = cmd.output() {
                    if output.status.success() {
                        let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
                        if !s.is_empty() { return Some(s); }
                    }
                }
            }
            None
        };

        // 2. user.name / user.email
        map.insert("user_name".into(), read_config(&repo_path, "user.name").into());
        map.insert("user_email".into(), read_config(&repo_path, "user.email").into());

        // 3. credential.helper
        map.insert("credential_helper".into(), read_config(&repo_path, "credential.helper").into());

        // 4. core.sshCommand（自定义 ssh 命令）
        map.insert("core_ssh_command".into(), read_config(&repo_path, "core.sshCommand").into());

        // 5. 常见 SSH 密钥路径是否存在
        let home_dir = dirs::home_dir();
        let mut ssh_keys: Vec<serde_json::Value> = Vec::new();
        let candidates = ["~/.ssh/id_ed25519", "~/.ssh/id_ed25519.pub",
                          "~/.ssh/id_rsa", "~/.ssh/id_rsa.pub",
                          "~/.ssh/id_ecdsa", "~/.ssh/id_ecdsa.pub"];
        for cand in candidates {
            let expanded = if let Some(ref home) = home_dir {
                let p = cand.trim_start_matches("~/");
                home.join(p).to_string_lossy().to_string()
            } else {
                cand.to_string()
            };
            let exists = std::path::Path::new(&expanded).exists();
            let mut obj = serde_json::Map::new();
            obj.insert("path".into(), expanded.into());
            obj.insert("exists".into(), serde_json::Value::Bool(exists));
            ssh_keys.push(serde_json::Value::Object(obj));
        }
        map.insert("ssh_keys".into(), serde_json::Value::Array(ssh_keys));

        // 6. GIT_SSH_COMMAND 环境变量（运行时覆盖）
        map.insert("git_ssh_command_env".into(),
            std::env::var("GIT_SSH_COMMAND").ok().into());
        map.insert("ssh_auth_sock_env".into(),
            std::env::var("SSH_AUTH_SOCK").ok().into());

        // 7. 本地完整 config（git config --local --list），透传方便调试
        let mut local_kv = serde_json::Map::new();
        if let Ok(output) = git_command()
            .arg("-C").arg(&repo_path)
            .arg("config").arg("--local").arg("--list")
            .output()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if let Some((k, v)) = line.split_once('=') {
                    local_kv.insert(k.to_string(), v.to_string().into());
                }
            }
        }
        map.insert("local_config".into(), serde_json::Value::Object(local_kv));

        Ok(serde_json::Value::Object(map))
    })
    .await
    .map_err(|e| format!("读取仓库配置异常: {}", e))?;

    result
}

#[command]
async fn save_credentials(repo_path: String, remote_name: String, username: String, token: String) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_read_guard();
        let repo = Repository::open(&repo_path).map_err(|e| e.to_string())?;
        let remote = repo.find_remote(&remote_name).map_err(|e| e.to_string())?;
        let url = remote.url().ok_or("无法获取远程 URL")?;

        // 配置 credential.helper 为 store（文件存储，明文保存在 ~/.git-credentials）
        git_command()
            .arg("-C").arg(&repo_path)
            .arg("config")
            .arg("--local")
            .arg("credential.helper")
            .arg("store")
            .output()
            .map_err(|e| format!("无法配置 credential helper: {}", e))?;

        // 使用 git credential approve 存储凭证
        let mut child = git_command()
            .arg("-C").arg(&repo_path)
            .arg("credential")
            .arg("approve")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("无法执行 git credential: {}", e))?;

        if let Some(stdin) = child.stdin.as_mut() {
            write!(stdin, "url={}\n", url).ok();
            write!(stdin, "username={}\n", username).ok();
            write!(stdin, "password={}\n", token).ok();
        }

        let output = child.wait_with_output().map_err(|e| format!("等待失败: {}", e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("存储凭证失败: {}", stderr));
        }

        Ok(())
    })
    .await
    .map_err(|e| format!("保存凭证失败: {}", e))?
}

#[command]
async fn checkout_branch(repo_path: String, branch_name: String, force: Option<bool>) -> Result<()> {
    // 切换分支涉及大量阻塞 I/O（git checkout 外部进程），改为在 blocking 线程池执行，
    // 避免阻塞 Tauri async runtime worker 线程导致 Windows WebView2 消息泵饿死、应用闪退。
    tokio::task::spawn_blocking(move || {
        let _guard = git_write_guard();
        let force_flag = force.unwrap_or(false);

        // Strategy 1: try direct checkout (for local branches, including those with "/" in name)
        let mut cmd = git_command();
        cmd.arg("-C").arg(&repo_path).arg("checkout");
        if force_flag {
            cmd.arg("-f");
        }
        cmd.arg(&branch_name);

        let output = cmd.output()
            .map_err(|e| format!("无法执行 git checkout 命令: {}", e))?;

        if output.status.success() {
            return Ok(());
        }

        // Strategy 1.5: If branch name contains "/", it might be "remote/branch" format
        // Try creating a local tracking branch
        if let Some((_remote, name)) = branch_name.split_once('/') {
            let mut track_cmd = git_command();
            track_cmd.arg("-C").arg(&repo_path).arg("checkout");
            if force_flag {
                track_cmd.arg("-f");
            }
            track_cmd.arg("-b").arg(name).arg("--track").arg(&branch_name);

            let track_output = track_cmd.output()
                .map_err(|e| format!("无法执行 git checkout 命令: {}", e))?;

            if track_output.status.success() {
                return Ok(());
            }
        }

        // Strategy 2: try checkout with --track for remote branches
        let remotes_cmd = git_command()
            .arg("-C").arg(&repo_path)
            .arg("remote").arg("-v")
            .output()
            .map_err(|e| format!("无法获取远程列表: {}", e))?;

        let remotes_output = String::from_utf8_lossy(&remotes_cmd.stdout);
        let mut all_remotes: Vec<String> = remotes_output.lines()
            .filter_map(|line| line.split_whitespace().next())
            .map(|s| s.to_string())
            .collect();

        if all_remotes.is_empty() {
            all_remotes.push("origin".to_string());
        }

        for remote in &all_remotes {
            let remote_branch = format!("{}/{}", remote, branch_name);
            let mut track_cmd = git_command();
            track_cmd.arg("-C").arg(&repo_path).arg("checkout");
            if force_flag {
                track_cmd.arg("-f");
            }
            track_cmd.arg("-b").arg(&branch_name).arg("--track").arg(&remote_branch);

            let track_output = track_cmd.output()
                .map_err(|e| format!("无法执行 git checkout 命令: {}", e))?;

            if track_output.status.success() {
                return Ok(());
            }
        }

        // If all strategies fail, return the error from the first attempt
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let msg = format!("{}{}", stderr, stdout);
        Err(if msg.trim().is_empty() { format!("分支 '{}' 不存在", branch_name) } else { msg })
    })
    .await
    .map_err(|e| format!("切换分支任务失败: {}", e))?
}

#[command]
fn checkout_remote_branch(repo_path: String, remote_branch: String) -> Result<()> {
    let _guard = git_write_guard();
    // remote_branch 形如 "origin/feature/login"，本地分支名取第一个 "/" 之后的部分
    let local_name = match remote_branch.split_once('/') {
        Some((_, name)) => name.to_string(),
        None => remote_branch.clone(),
    };

    // 若本地已存在同名分支，直接切换过去即可
    let local_exists = git_command()
        .arg("-C").arg(&repo_path)
        .arg("show-ref").arg("--verify").arg("--quiet")
        .arg(format!("refs/heads/{}", local_name))
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if local_exists {
        let out = git_command()
            .arg("-C").arg(&repo_path)
            .arg("checkout").arg(&local_name)
            .output()
            .map_err(|e| format!("无法执行 git checkout 命令: {}", e))?;
        if out.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        let msg = format!("{}{}", stderr, stdout);
        return Err(if msg.trim().is_empty() { format!("切换分支 '{}' 失败", local_name) } else { msg });
    }

    // 否则创建本地跟踪分支并切换
    let out = git_command()
        .arg("-C").arg(&repo_path)
        .arg("checkout").arg("-b").arg(&local_name).arg("--track").arg(&remote_branch)
        .output()
        .map_err(|e| format!("无法执行 git checkout 命令: {}", e))?;

    if out.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        let msg = format!("{}{}", stderr, stdout);
        Err(if msg.trim().is_empty() { format!("检出远程分支 '{}' 失败", remote_branch) } else { msg })
    }
}

#[command]
async fn pull_branch(repo_path: String) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_write_guard();

        // 极简仓库（刚 init 还没有任何 remote）：直接短路返回特定错误前缀，
        // 前端识别后弹「创建远程」对话框引导用户添加 remote，避免暴露 git 原始报错。
        // （用 libgit2 直接判断 RemoteNames.len()，比依赖 `git fetch --all` 的 stderr 文案更可靠）
        {
            let probe = Repository::open(&repo_path).map_err(|e| e.to_string())?;
            let remotes = probe.remotes().map_err(|e| e.to_string())?;
            if remotes.is_empty() {
                return Err("NO_REMOTE: 当前仓库没有配置任何远程仓库".to_string());
            }
        }

        // 先 fetch 远程最新代码
        let fetch_output = git_command()
            .arg("-C")
            .arg(&repo_path)
            .arg("fetch")
            .arg("--all")
            .arg("--no-progress")
            .env("GIT_TERMINAL_PROMPT", "0")  // 禁用交互式凭证提示，防止无终端时挂起
            .output()
            .map_err(|e| format!("无法执行 git fetch 命令: {}", e))?;

        if !fetch_output.status.success() {
            let stderr = String::from_utf8_lossy(&fetch_output.stderr);
            let stdout = String::from_utf8_lossy(&fetch_output.stdout);
            let mut msg = format!("{}{}", stderr, stdout);
            if msg.trim().is_empty() {
                msg = "获取远程代码失败".to_string();
            }
            if msg.contains("Authentication failed") || msg.contains("could not read Username") || msg.contains("terminal prompts disabled") {
                msg = format!("拉取失败：远程仓库认证失败\n\n请确保已配置认证方式（SSH 密钥或 credential helper）。\n\n原始错误:\n{}", msg);
            }
            return Err(msg);
        }

        // 再尝试 fast-forward merge
        let output = git_command()
            .arg("-C")
            .arg(&repo_path)
            .arg("pull")
            .arg("--ff-only")
            .arg("--no-progress")
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .map_err(|e| format!("无法执行 git pull 命令: {}", e))?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // 返回拉取结果信息
            if stdout.trim().is_empty() {
                Ok(())
            } else {
                // 即使成功也返回信息（如 "Already up to date."）
                Ok(())
            }
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let msg = format!("{}{}", stderr, stdout);
            Err(if msg.trim().is_empty() { "拉取失败".to_string() } else { msg })
        }
    })
    .await
    .map_err(|e| format!("拉取失败: {}", e))?
}

/// 添加远程仓库（`git remote add <name> <url>`）。
/// 用于「拉取无远程」对话框的"创建远程"按钮。
/// - `name`：远程简称，默认 `origin`（业务层兜底）
/// - `url`：远程仓库 URL（https/ssh/git 协议均可），非空校验由业务层完成
#[command]
async fn add_remote(repo_path: String, name: String, url: String) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_write_guard();

        // 名称兜底：业务层必传，但多一层防御避免出现裸 `git remote add  <url>`（无名）
        let name = if name.trim().is_empty() { "origin".to_string() } else { name.trim().to_string() };

        let output = git_command()
            .arg("-C")
            .arg(&repo_path)
            .arg("remote")
            .arg("add")
            .arg(&name)
            .arg(&url)
            .output()
            .map_err(|e| format!("无法执行 git remote add 命令: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let mut msg = format!("{}{}", stderr, stdout);
            if msg.trim().is_empty() {
                msg = "添加远程仓库失败".to_string();
            }
            return Err(msg);
        }
        Ok(())
    })
    .await
    .map_err(|e| format!("添加远程仓库失败: {}", e))?
}

/// 克隆远程仓库到本地目录。
/// - `url`：远程仓库地址（https/ssh/git 协议均可）
/// - `target_dir`：最终目标目录（不存在则 git 自动创建；已存在且必须为空）
/// - `branch`：可选，指定检出分支；空字符串表示使用远程默认分支
/// - `include_submodules`：是否同时初始化并拉取子模块（--recurse-submodules）
/// - `fetch_tags`：是否获取所有标签（--tags）
/// 返回克隆后的本地绝对路径（与传入 target_dir 一致）
#[command]
async fn clone_repository(
    url: String,
    target_dir: String,
    branch: String,
    include_submodules: bool,
    fetch_tags: bool,
) -> Result<String> {
    let target_path = std::path::Path::new(&target_dir);
    // 目标已存在且非空 → 明确拒绝（git clone 自己也会拒绝，我们提前给友好错误）
    if target_path.exists() {
        if !target_path.is_dir() {
            return Err(format!("目标路径已存在且不是文件夹：{}", target_path.display()));
        }
        let is_empty = target_path
            .read_dir()
            .map(|mut r| r.next().is_none())
            .unwrap_or(false);
        if !is_empty {
            return Err(format!(
                "目标目录已存在且非空，请选择一个空目录或不存在的路径：{}",
                target_path.display()
            ));
        }
    }
    // 父目录必须已存在（git clone 只能创建最后一级，不能创建多级不存在的父目录）
    if let Some(parent) = target_path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            return Err(format!("目标父目录不存在：{}", parent.display()));
        }
    }

    let url_clone = url.clone();
    let dir_clone = target_dir.clone();
    let branch_clone = branch.clone();
    let result = tokio::task::spawn_blocking(move || {
        let _guard = git_write_guard();
        let mut cmd = git_command();
        cmd.env("GIT_TERMINAL_PROMPT", "0");
        // 注意：先 "clone" 子命令本身，再跟 clone 专属选项
        // （放在 "clone" 前面的选项会被 git 主命令解析，而 --branch/--recurse-submodules 等是 clone 专属的）
        cmd.arg("clone");
        if !branch_clone.is_empty() {
            cmd.arg("--branch").arg(&branch_clone);
        }
        if include_submodules {
            cmd.arg("--recurse-submodules");
        }
        if fetch_tags {
            cmd.arg("--tags");
        }
        cmd.arg(&url_clone).arg(&dir_clone);

        let output = cmd
            .output()
            .map_err(|e| format!("无法执行 git clone 命令：{}", e))?;

        if output.status.success() {
            Ok(dir_clone)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let mut msg = format!("{}{}", stderr, stdout);
            if msg.trim().is_empty() {
                msg = "克隆失败".to_string();
            }
            if msg.contains("Authentication failed")
                || msg.contains("could not read Username")
                || msg.contains("terminal prompts disabled")
                || msg.contains("Permission denied")
            {
                msg = format!(
                    "克隆失败：远程仓库认证失败。\n请确保已配置认证方式（SSH 密钥或 credential helper）。\n\n原始错误:\n{}",
                    msg
                );
            }
            Err(msg)
        }
    })
    .await
    .map_err(|e| format!("克隆任务异常: {}", e))?;

    result
}

#[command]
async fn create_branch(repo_path: String, branch_name: String) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_write_guard();
        let repo = Repository::open(&repo_path).map_err(|e| e.to_string())?;

        let head = repo.head().map_err(|e| e.to_string())?;
        let target = head.target().ok_or("Cannot get head target")?;

        repo.branch(&branch_name, &repo.find_commit(target).map_err(|e| e.to_string())?, false)
            .map_err(|e| e.to_string())?;

        Ok(())
    })
    .await
    .map_err(|e| format!("创建分支失败: {}", e))?
}

#[command]
async fn get_current_branch(repo_path: String) -> Result<String> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_read_guard();
        let repo = Repository::open(&repo_path).map_err(|e| e.to_string())?;

        let head = repo.head().map_err(|e| e.to_string())?;
        Ok(head.shorthand().unwrap_or("detached").to_string())
    })
    .await
    .map_err(|e| format!("获取当前分支失败: {}", e))?
}

#[command]
async fn merge_branch(repo_path: String, branch_name: String) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_write_guard();
        // 将指定分支合并到当前分支（git 常规语义，不切换分支）
        let output = git_command()
            .arg("-C")
            .arg(&repo_path)
            .arg("merge")
            .arg("--no-progress")
            .arg("--no-edit")
            .arg(&branch_name)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .map_err(|e| format!("无法执行 git 命令: {}", e))?;

        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let msg = format!("{}{}", stderr, stdout);
            Err(if msg.trim().is_empty() { "合并失败".to_string() } else { msg })
        }
    })
    .await
    .map_err(|e| format!("合并分支失败: {}", e))?
}

#[command]
async fn rename_branch(repo_path: String, old_name: String, new_name: String) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_write_guard();
        let output = git_command()
            .arg("-C")
            .arg(&repo_path)
            .arg("branch")
            .arg("-m")
            .arg(&old_name)
            .arg(&new_name)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .map_err(|e| format!("无法执行 git 命令: {}", e))?;

        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let msg = format!("{}{}", stderr, stdout);
            Err(if msg.trim().is_empty() { "重命名分支失败".to_string() } else { msg })
        }
    })
    .await
    .map_err(|e| format!("重命名分支失败: {}", e))?
}

/// 删除本地分支。
/// - delete_tracking：同时删除本地远程跟踪引用（如 refs/remotes/origin/gsj_new）
/// - delete_remote：同时用 `git push <remote> :<remote_branch>` 删除远程分支
/// 仅当分支设置了上游（upstream）时，后两者才有意义。
#[command]
async fn delete_branch(
    repo_path: String,
    branch_name: String,
    delete_tracking: bool,
    delete_remote: bool,
) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_write_guard();
        let repo = Repository::open(&repo_path).map_err(|e| e.to_string())?;

        // 不允许删除当前检出的分支
        if let Ok(head) = repo.head() {
            if let Some(hb) = head.shorthand() {
                if hb == branch_name {
                    return Err("不能删除当前检出的分支，请先切换到其它分支".to_string());
                }
            }
        }

        // 删除本地分支前先取上游信息（删除后联动关系即消失）
        let upstream_name: Option<String> = repo
            .find_branch(&branch_name, BranchType::Local)
            .ok()
            .and_then(|b| b.upstream().ok())
            .and_then(|u| u.name().ok().flatten().map(|n| n.to_string()));

        // 1. 删除本地分支
        let mut branch = repo
            .find_branch(&branch_name, BranchType::Local)
            .map_err(|e| format!("找不到本地分支 {}: {}", branch_name, e))?;
        branch.delete().map_err(|e| {
            let msg = e.to_string();
            if msg.contains("not fully merged") {
                format!("分支 {} 尚未完全合并，无法安全删除（请先合并或改用强制删除）", branch_name)
            } else {
                format!("删除本地分支失败: {}", msg)
            }
        })?;

        // 2. 可选：删除本地远程跟踪引用（如 origin/gsj_new）
        if delete_tracking {
            if let Some(ref_name) = &upstream_name {
                // 远程跟踪分支可能以 BranchType::Remote 形式存在，尽力删除，失败不阻断主流程
                let _ = repo
                    .find_branch(ref_name, BranchType::Remote)
                    .and_then(|mut b| b.delete());
            }
        }

        // 3. 可选：从远程删除分支
        if delete_remote {
            match &upstream_name {
                Some(ref_name) => {
                    if let Some((remote, remote_branch)) = ref_name.split_once('/') {
                        let output = git_command()
                            .arg("-C")
                            .arg(&repo_path)
                            .arg("push")
                            .arg("--no-progress")
                            .arg(remote)
                            .arg(format!(":{}", remote_branch))
                            .env("GIT_TERMINAL_PROMPT", "0")
                            .output()
                            .map_err(|e| format!("无法执行 git push 命令: {}", e))?;
                        if output.status.success() {
                            Ok(())
                        } else {
                            let stderr = String::from_utf8_lossy(&output.stderr);
                            let stdout = String::from_utf8_lossy(&output.stdout);
                            let mut msg = format!("{}{}", stderr, stdout);
                            if msg.trim().is_empty() {
                                msg = "从远程删除分支失败（未知错误）".to_string();
                            }
                            if msg.contains("Authentication failed")
                                || msg.contains("could not read Username")
                                || msg.contains("terminal prompts disabled")
                            {
                                msg = format!(
                                    "从远程删除分支失败：远程仓库认证失败\n\n请确保已配置认证方式（SSH 密钥或 credential helper）。\n\n原始错误:\n{}",
                                    msg
                                );
                            }
                            Err(msg)
                        }
                    } else {
                        Err("无法解析上游分支名称，无法从远程删除".to_string())
                    }
                }
                None => Err("该分支没有设置上游，无法从远程删除".to_string()),
            }
        } else {
            Ok(())
        }
    })
    .await
    .map_err(|e| format!("删除分支失败: {}", e))?
}

/// 删除远程分支（从远端移除）+ 可选删除本地远程跟踪引用。
/// 通过 `git push <remote> :<remote_branch>` 实现，自动复用系统 git 的认证配置。
#[command]
async fn delete_remote_branch(
    repo_path: String,
    remote: String,
    remote_branch: String,
    delete_tracking: bool,
) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_write_guard();

        // 1. 从远程删除分支
        let output = git_command()
            .arg("-C")
            .arg(&repo_path)
            .arg("push")
            .arg("--no-progress")
            .arg(&remote)
            .arg(format!(":{}", remote_branch))
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .map_err(|e| format!("无法执行 git push 命令: {}", e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let mut msg = format!("{}{}", stderr, stdout);
            if msg.trim().is_empty() {
                msg = "从远程删除分支失败（未知错误）".to_string();
            }
            if msg.contains("Authentication failed")
                || msg.contains("could not read Username")
                || msg.contains("terminal prompts disabled")
            {
                msg = format!(
                    "从远程删除分支失败：远程仓库认证失败\n\n请确保已配置认证方式（SSH 密钥或 credential helper）。\n\n原始错误:\n{}",
                    msg
                );
            }
            return Err(msg);
        }

        // 2. 可选：删除本地远程跟踪引用（如 refs/remotes/origin/gsj_new）
        if delete_tracking {
            let repo = Repository::open(&repo_path).map_err(|e| e.to_string())?;
            let tracking_name = format!("{}/{}", remote, remote_branch);
            let _ = repo
                .find_branch(&tracking_name, BranchType::Remote)
                .and_then(|mut b| b.delete());
        }

        Ok(())
    })
    .await
    .map_err(|e| format!("删除远程分支失败: {}", e))?
}

/// 探测远程仓库的默认分支名。
/// 1. 优先读本地 `refs/remotes/<remote>/HEAD` 符号引用（`git clone` 时自动创建），
///    其目标形如 `refs/remotes/origin/main`，取最后一段即默认分支名。
/// 2. 若本地符号引用不存在（老仓库 / 非标准 clone / 手动 fetch），用
///    `git ls-remote --symref <remote> HEAD` 查询服务端（只读查询，GitHub/GitLab/GitCode 都返回）。
///    ls-remote 失败或 3s 超时则返回 None，不阻塞主流程。
fn detect_default_branch(repo_path: &str, remote: &str) -> Option<String> {
    // 1. 本地符号引用
    if let Ok(repo) = Repository::open(repo_path) {
        if let Ok(head_ref) = repo.find_reference(&format!("refs/remotes/{}/HEAD", remote)) {
            if let Some(target) = head_ref.symbolic_target() {
                if let Some(name) = target.rsplit('/').next() {
                    if !name.is_empty() {
                        return Some(name.to_string());
                    }
                }
            }
        }
    }

    // 2. ls-remote --symref 兜底
    let mut child = std::process::Command::new("git")
        .args(["-C", repo_path, "ls-remote", "--symref", remote, "HEAD"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;

    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(3);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                let mut buf = String::new();
                if let Some(mut out) = child.stdout.take() {
                    use std::io::Read;
                    let _ = out.read_to_string(&mut buf);
                }
                // 解析 "ref: refs/heads/<name>\tHEAD"
                for line in buf.lines() {
                    if let Some(rest) = line.strip_prefix("ref: refs/heads/") {
                        if let Some(name) = rest.split_whitespace().next() {
                            if !name.is_empty() {
                                return Some(name.to_string());
                            }
                        }
                    }
                }
                return None;
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(_) => return None,
        }
    }
}

/// 检测远程分支是否可被当前用户删除。
/// 见 `RemoteDeleteCheck` 注释。
#[command]
async fn check_remote_branch_deletable(
    repo_path: String,
    remote: String,
    remote_branch: String,
) -> Result<RemoteDeleteCheck> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_read_guard();

        // 1. 协议快速检查：git:// 协议只读不可写。
        //    使用 libgit2 读取 remote url / pushurl（pushurl 优先，对应 `remote.<name>.pushurl`）。
        if let Ok(repo) = Repository::open(&repo_path) {
            if let Ok(r) = repo.find_remote(&remote) {
                let push_url = r
                    .pushurl()
                    .or_else(|| r.url())
                    .unwrap_or("")
                    .to_string();
                let lower = push_url.to_lowercase();
                if lower.starts_with("git://") {
                    return Ok(RemoteDeleteCheck {
                        can_delete: false,
                        reason: Some(format!(
                            "远程 {} 使用只读 git:// 协议，无法推送删除",
                            push_url
                        )),
                    });
                }
            }
        }

        // 2. 默认分支检测（本地符号引用 + ls-remote --symref 兜底，确定性）：
        //    默认分支在 GitHub/GitLab/GitCode 等平台几乎必然受保护，直接判不可删。
        //    本步骤不依赖服务端 push 校验（很多服务端在 dry-run 阶段不响应保护检查），
        //    因此对 GitCode 这类服务端也可靠。
        if let Some(default) = detect_default_branch(&repo_path, &remote) {
            if default == remote_branch {
                return Ok(RemoteDeleteCheck {
                    can_delete: false,
                    reason: Some(format!(
                        "{} 是远程 {} 的默认分支（服务端认定），无法删除",
                        remote_branch, remote
                    )),
                });
            }
        }

        // 3. 试运行 `git push --dry-run <remote> :<remote_branch>`，
        //    通过 `GIT_TERMINAL_PROMPT=0` 禁用交互式凭证避免长时间挂起。
        //    真正执行 `git push` 时 `delete_branch` / `delete_remote_branch` 仍会处理认证提示。
        //    注意：部分服务端（如 GitCode/GitLab）在 dry-run 阶段不校验保护规则，
        //    因此此步主要作为"非默认受保护分支"的补充探测。
        let output = git_command()
            .arg("-C")
            .arg(&repo_path)
            .arg("push")
            .arg("--dry-run")
            .arg("--no-verify")
            .arg("--no-progress")
            .arg(&remote)
            .arg(format!(":{}", remote_branch))
            .env("GIT_TERMINAL_PROMPT", "0")
            .output();

        match output {
            Ok(out) if out.status.success() => Ok(RemoteDeleteCheck {
                can_delete: true,
                reason: None,
            }),
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                let stdout = String::from_utf8_lossy(&out.stdout);
                let msg = format!("{}{}", stderr, stdout).to_lowercase();

                // 服务端明确拒绝：分支受保护 / 无权限
                let denied_kw = [
                    "protected", "denied", "forbidden", "permission denied",
                    "you do not have permission", "cannot delete", "can't delete",
                    "not allowed to delete", "not allowed to push", "you are not allowed",
                    "protected branch", "branch protection", "403",
                ];
                for kw in denied_kw.iter() {
                    if msg.contains(kw) {
                        return Ok(RemoteDeleteCheck {
                            can_delete: false,
                            reason: Some(
                                "远程分支受保护或您没有删除权限（dry-run 已拒绝）".to_string(),
                            ),
                        });
                    }
                }

                // 其他错误（认证失败、网络不通、找不到分支等）视为"未知"，让用户试一次
                Ok(RemoteDeleteCheck {
                    can_delete: true,
                    reason: None,
                })
            }
            Err(_) => Ok(RemoteDeleteCheck {
                can_delete: true,
                reason: None,
            }),
        }
    })
    .await
    .map_err(|e| format!("检测远程分支可删除性失败: {}", e))?
}

#[command]
fn open_folder_dialog() -> Result<Option<String>> {
    let _guard = git_read_guard();
    let result = rfd::FileDialog::new().pick_folder();

    match result {
        Some(path) => Ok(Some(path.to_string_lossy().to_string())),
        None => Ok(None),
    }
}

#[command]
fn open_file_dialog() -> Result<Option<String>> {
    let _guard = git_read_guard();
    match rfd::FileDialog::new().pick_file() {
        Some(path) => Ok(Some(path.to_string_lossy().to_string())),
        None => Ok(None),
    }
}

#[command]
async fn set_ssh_key_path(repo_path: String, key_path: String) -> Result<String> {
    let _guard = git_read_guard();
    let key_path = key_path.trim().to_string();
    if key_path.is_empty() {
        // 清空：移除 core.sshCommand（忽略失败，可能本来就没设）
        let _ = git_command()
            .arg("-C").arg(&repo_path)
            .arg("config").arg("--local").arg("--unset")
            .arg("core.sshCommand")
            .output();
        return Ok(String::new());
    }
    // 写入标准 ssh 私钥指定命令：仅用该私钥，避免 ssh-agent 里其他 key 干扰
    let cmd = format!("ssh -i \"{}\" -o IdentitiesOnly=yes", key_path);
    let status = git_command()
        .arg("-C").arg(&repo_path)
        .arg("config").arg("--local")
        .arg("core.sshCommand").arg(&cmd)
        .status();
    match status {
        Ok(s) if s.success() => Ok(cmd),
        Ok(s) => Err(format!("git config 写入失败，退出码: {}", s)),
        Err(e) => Err(format!("执行 git 失败: {}", e)),
    }
}

// 检测系统是否可用 git：优先 PATH，Windows 额外探测常见安装路径。
// 返回 { available, version, path }，供前端判断是否需要引导用户安装 Git。
#[command]
fn detect_git() -> Result<serde_json::Value> {
    let candidates: Vec<String> = if cfg!(target_os = "windows") {
        vec![
            "git".into(),
            "C:\\Program Files\\Git\\cmd\\git.exe".into(),
            "C:\\Program Files (x86)\\Git\\cmd\\git.exe".into(),
            "C:\\Program Files\\Git\\bin\\git.exe".into(),
        ]
    } else {
        vec!["git".into(), "/usr/bin/git".into()]
    };
    let os_str = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    };
    for c in candidates {
        if let Ok(out) = std::process::Command::new(&c).arg("--version").output() {
            if out.status.success() {
                let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
                // 形如 "git version 2.45.1.windows.1"
                let version = raw
                    .strip_prefix("git version ")
                    .unwrap_or(&raw)
                    .to_string();
                return Ok(serde_json::json!({
                    "available": true,
                    "version": version,
                    "path": c,
                    "os": os_str,
                }));
            }
        }
    }
    Ok(serde_json::json!({
        "available": false,
        "version": "",
        "path": "",
        "os": os_str,
    }))
}

// ===== 项目扫描 =====

fn get_scan_directories() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    let home_path = PathBuf::from(&home);

    // 用户主目录下的常见开发目录（剔除 Desktop / Downloads 等非代码根：
    // 这类目录体量巨大且无仓库意义，深度递归扫描会严重拖慢首屏）
    let sub_dirs = [
        "workspace", "code", "repos", "projects",
        "opensource", "sandbox", "dev", "github",
        "gitee", "source",
        "Documents\\site", "Documents\\projects",
        "Documents\\code", "Documents\\workspace",
    ];

    for sub in &sub_dirs {
        let p = home_path.join(sub);
        if p.exists() {
            dirs.push(p);
        }
    }

    // 当前项目目录的上级
    if let Ok(current) = std::env::current_dir() {
        if let Some(parent) = current.parent() {
            dirs.push(parent.to_path_buf());
        }
    }

    dirs
}

// Windows 上 std::fs::canonicalize 会返回带 \\?\ 前缀的扩展长度路径，
// 展示给用户前需还原成常规路径（\\?\C:\... → C:\...；\\?\UNC\... → \\...）。
fn normalize_windows_path(p: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let s = p.to_string_lossy();
        if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{}", rest));
        }
        if let Some(rest) = s.strip_prefix(r"\\?\") {
            return PathBuf::from(rest);
        }
    }
    p.to_path_buf()
}

fn is_skip_dir(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(
        lower.as_str(),
        "node_modules" | "vendor" | "target" | "build" | "dist" | "__pycache__"
        | ".git" | ".svn" | ".hg" | "bin" | "obj" | ".next" | ".nuxt"
        | ".cache" | ".gradle" | ".idea" | ".vscode" | ".vs"
        | "windows" | "program files" | "program files (x86)" | "programdata"
        | "system volume information" | "$recycle.bin" | "temp"
        | "appdata" | "locallow" | "local" | "roaming"
        | "tencent" | "wetype"
    ) || lower.starts_with("$")
       || lower.starts_with(".")
}

fn scan_directory(
    dir: &Path,
    current_depth: usize,
    max_depth: usize,
    projects: &mut Vec<ScannedProject>,
    visited: &mut std::collections::HashSet<PathBuf>,
) {
    if current_depth > max_depth {
        return;
    }

    let canonical = match dir.canonicalize() {
        Ok(c) => c,
        Err(_) => return,
    };

    if visited.contains(&canonical) {
        return;
    }
    visited.insert(canonical.clone());

    // 检查是否是 Git 仓库
    if canonical.join(".git").exists() {
        let name = canonical
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());
        projects.push(ScannedProject {
            path: normalize_windows_path(&canonical).to_string_lossy().to_string(),
            name,
            vcs_type: "git".to_string(),
        });
        return; // Git 仓库内部不再递归
    }

    // 检查是否是 SVN 仓库
    if canonical.join(".svn").exists() {
        let name = canonical
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());
        projects.push(ScannedProject {
            path: normalize_windows_path(&canonical).to_string_lossy().to_string(),
            name,
            vcs_type: "svn".to_string(),
        });
        return;
    }

    // 递归扫描子目录
    let entries = match fs::read_dir(&canonical) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let dir_name = match path.file_name() {
            Some(n) => n.to_string_lossy().to_string(),
            None => continue,
        };

        if is_skip_dir(&dir_name) {
            continue;
        }

        scan_directory(&path, current_depth + 1, max_depth, projects, visited);
    }
}

#[command]
fn scan_projects() -> Result<Vec<ScannedProject>> {
    let _guard = git_read_guard();
    let mut projects = Vec::new();
    let scan_dirs = get_scan_directories();
    let mut visited = std::collections::HashSet::new();

    for base_dir in &scan_dirs {
        scan_directory(base_dir, 1, 4, &mut projects, &mut visited);
    }

    // 去重
    let mut seen = std::collections::HashSet::new();
    projects.retain(|p| seen.insert(p.path.clone()));
    projects.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    Ok(projects)
}

// ===== 文件树 =====

fn build_file_tree(
    dir: &Path,
    base_path: &Path,
    depth: usize,
    max_depth: usize,
) -> Vec<FileTreeNode> {
    if depth > max_depth {
        return Vec::new();
    }

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut nodes = Vec::new();

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();
        let name = match path.file_name() {
            Some(n) => n.to_string_lossy().to_string(),
            None => continue,
        };

        // 跳过 VCS 目录和构建产物
        if is_skip_dir(&name) {
            continue;
        }

        let is_dir = path.is_dir();
        let relative_path = path
            .strip_prefix(base_path)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");

        let children = if is_dir {
            build_file_tree(&path, base_path, depth + 1, max_depth)
        } else {
            Vec::new()
        };

        nodes.push(FileTreeNode {
            name,
            path: relative_path,
            is_dir,
            children,
        });
    }

    // 文件夹优先，然后按名称排序
    nodes.sort_by(|a, b| {
        match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        }
    });

    nodes
}

#[command]
async fn get_file_tree(repo_path: String) -> Result<Vec<FileTreeNode>> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_read_guard();
        let base = PathBuf::from(&repo_path);
        if !base.exists() {
            return Err("Repository path does not exist".to_string());
        }
        Ok(build_file_tree(&base, &base, 1, 5))
    })
    .await
    .map_err(|e| format!("加载文件树失败: {}", e))?
}

// ===== 文件差异 =====

/// git 判定二进制的启发式：前 8000 字节内出现 NUL 字节即视为二进制
/// （与 Git 的 buffer_is_binary 一致，避免把 exe/图片/zip 当文本逐行 diff 出乱码）
fn is_binary_bytes(bytes: &[u8]) -> bool {
    const CHECK_LEN: usize = 8000;
    let end = bytes.len().min(CHECK_LEN);
    bytes[..end].contains(&0)
}

/// 读取工作区文件内容（按行），并判定是否为二进制。
/// 返回 (行内容, 是否二进制)；二进制时行内容为空。
/// 非 UTF-8 但不含 NUL（如 GBK 文本）按 lossy 转换，保证内容可见而不是整片空白。
fn read_file_lines(path: &Path) -> (Vec<String>, bool) {
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(_) => return (Vec::new(), false),
    };
    if is_binary_bytes(&bytes) {
        return (Vec::new(), true);
    }
    match String::from_utf8(bytes) {
        Ok(content) => (content.lines().map(|l| l.to_string()).collect(), false),
        Err(e) => (
            String::from_utf8_lossy(e.as_bytes())
                .lines()
                .map(|l| l.to_string())
                .collect(),
            false,
        ),
    }
}

/// Myers 差分的一步编辑操作（按旧文件行推进）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffOp {
    Equal,
    Delete,
    Insert,
}

/// Myers O(ND) 差分：返回从 `old` 变换到 `new` 的编辑脚本。
/// 采用经典的「贪心蛇 + 对角线搜索」实现（Eugene W. Myers 1986，`git diff` 同款核心思路）。
/// 空间复杂度 O(N+M)（只存 V 数组 + 回溯 trace），远优于旧版 O(N·M) DP 表。
fn myers_diff(old: &[String], new: &[String]) -> Vec<DiffOp> {
    let n = old.len();
    let m = new.len();

    // 空文件特判：避免 V 数组的 k±1 越界（n=0 时 max 很小仍会访问 k+1）
    if n == 0 && m == 0 {
        return Vec::new();
    }
    if n == 0 {
        return vec![DiffOp::Insert; m];
    }
    if m == 0 {
        return vec![DiffOp::Delete; n];
    }

    let max = n + m;

    // V[k] 记录「第 d 步」沿对角线 k = x - y 能到达的最远 x。
    // 用滚动的一维数组，避免 O(D·(N+M)) 的完整 trace 存储；但回溯需要每步的快照，
    // 因此这里存 trace：trace[d][k] = 到达该步时对角线 k 的最远 x。
    // 长度 +2 留出 k±1 边界缓冲，避免 k = ±d 时访问越界。
    let mut v = vec![0isize; 2 * max + 3];
    // offset 让负对角线也能索引：k ∈ [-max, max] → idx = k + max + 1
    let offset = (max + 1) as isize;

    // trace：每一步保存整个 V 的拷贝（只有 O(D) 步，D 通常远小于 N+M）
    let mut trace: Vec<Vec<isize>> = Vec::new();

    for d in 0..=max {
        trace.push(v.clone());
        let d = d as isize;
        let mut k = -d;
        while k <= d {
            // 决定走哪条路径：向下（删除，x+1）还是向右（插入，y+1）
            let idx = (k + offset) as usize;
            let mut x = if k == -d || (k != d && v[idx - 1] < v[idx + 1]) {
                // 向下走：从 k+1 的对角线，x = v[k+1]（删除：x 不变时 y 增）
                v[idx + 1]
            } else {
                // 向右走：从 k-1 的对角线，x = v[k-1] + 1（插入）
                v[idx - 1] + 1
            };

            let mut y = x - k;

            // 贪心蛇：沿对角线尽量吃相同行
            while (x as usize) < n && (y as usize) < m && old[x as usize] == new[y as usize] {
                x += 1;
                y += 1;
            }

            v[idx] = x;

            if (x as usize) >= n && (y as usize) >= m {
                // 到达终点 (n, m)，回溯生成编辑脚本
                return backtrack(&trace, old, new, d as usize);
            }

            k += 2;
        }
    }

    // 理论上不会到这里（max = n+m 一定可达）
    Vec::new()
}

/// 根据 trace 从终点回溯出编辑脚本。
fn backtrack(
    trace: &[Vec<isize>],
    old: &[String],
    new: &[String],
    final_d: usize,
) -> Vec<DiffOp> {
    let n = old.len();
    let m = new.len();
    let max = n + m;
    let offset = (max + 1) as isize; // 与 myers_diff 的 v 索引体系一致（含 k±1 缓冲）

    let mut ops: Vec<DiffOp> = Vec::new();
    let mut x = n as isize;
    let mut y = m as isize;

    for d in (0..=final_d).rev() {
        let v = &trace[d];
        let k = x - y;
        let idx = (k + offset) as usize;

        // 判断这一步是从哪个方向进来的
        let prev_k = if k == -(d as isize)
            || (k != d as isize && v[idx - 1] < v[idx + 1])
        {
            k + 1 // 从「向下删除」方向来
        } else {
            k - 1 // 从「向右插入」方向来
        };

        let prev_idx = (prev_k + offset) as usize;
        let prev_x = v[prev_idx];
        let prev_y = prev_x - prev_k;

        // 先回退贪心蛇吃掉的相同行（Equal）
        while x > prev_x && y > prev_y {
            ops.push(DiffOp::Equal);
            x -= 1;
            y -= 1;
        }

        if d == 0 {
            break;
        }

        if x == prev_x {
            // 这一步是插入（x 不变，y 增）
            ops.push(DiffOp::Insert);
            y -= 1;
        } else {
            // 这一步是删除（y 不变，x 增）
            ops.push(DiffOp::Delete);
            x -= 1;
        }
    }

    // 起点处可能还有剩余的相同行
    while x > 0 && y > 0 {
        ops.push(DiffOp::Equal);
        x -= 1;
        y -= 1;
    }

    ops.reverse();
    ops
}

fn lcs_diff(old: &[String], new: &[String]) -> Vec<DiffLine> {
    // Myers 差分算法（git 同款）：时间复杂度 O((N+M)·D)，D 为实际差异量；
    // 空间复杂度 O(N+M) 线性（相比旧版 O(N·M) 全量 DP 表，几万行不再 OOM）。
    // 两个文件越接近（D 越小）跑得越快；完全不相关的最坏情况 O((N+M)^2)，但
    // 由 get_file_diff 的 MAX_DIFF_LINES 硬上限兜底，不会真正卡死。
    let ops = myers_diff(old, new);

    // 回溯生成 DiffLine（old_line/new_line 用 1-based 行号）
    let mut lines: Vec<DiffLine> = Vec::with_capacity(ops.len());
    let mut old_line: usize = 1;
    let mut new_line: usize = 1;

    for op in &ops {
        match op {
            DiffOp::Equal => {
                lines.push(DiffLine {
                    line_type: "context".to_string(),
                    content: old[old_line - 1].clone(),
                    new_content: String::new(),
                    old_line: Some(old_line),
                    new_line: Some(new_line),
                    old_segments: Vec::new(),
                    new_segments: Vec::new(),
                });
                old_line += 1;
                new_line += 1;
            }
            DiffOp::Delete => {
                lines.push(DiffLine {
                    line_type: "delete".to_string(),
                    content: old[old_line - 1].clone(),
                    new_content: String::new(),
                    old_line: Some(old_line),
                    new_line: None,
                    old_segments: Vec::new(),
                    new_segments: Vec::new(),
                });
                old_line += 1;
            }
            DiffOp::Insert => {
                lines.push(DiffLine {
                    line_type: "add".to_string(),
                    content: new[new_line - 1].clone(),
                    new_content: String::new(),
                    old_line: None,
                    new_line: Some(new_line),
                    old_segments: Vec::new(),
                    new_segments: Vec::new(),
                });
                new_line += 1;
            }
        }
    }

    // 后处理：将相邻的 delete + add 合并为 modified 行，做行内 diff
    let mut merged: Vec<DiffLine> = Vec::new();
    let mut idx = 0;
    while idx < lines.len() {
        let line = &lines[idx];
        if line.line_type == "delete"
            && idx + 1 < lines.len()
            && lines[idx + 1].line_type == "add"
        {
            let (old_segments, new_segments) = diff_chars(&line.content, &lines[idx + 1].content);
            merged.push(DiffLine {
                line_type: "modified".to_string(),
                content: line.content.clone(),
                new_content: lines[idx + 1].content.clone(),
                old_line: line.old_line,
                new_line: lines[idx + 1].new_line,
                old_segments,
                new_segments,
            });
            idx += 2;
        } else {
            merged.push(line.clone());
            idx += 1;
        }
    }

    merged
}

// 对两行文本做字符级 LCS，返回旧/新两侧的分段（changed 标记不同的部分）
fn diff_chars(old: &str, new: &str) -> (Vec<DiffSegment>, Vec<DiffSegment>) {
    // 超长行防护：字符级 LCS 是 O(m*n)，单行超长（压缩 js / 生成产物一行几万字符）会卡死/OOM。
    // 超阈值时放弃字符级标注，两侧各返回整行一个 changed=false 段 —— 行级底色（modified 灰底）
    // 仍表达"这行有改动"，但不会误标整行几十万字符全变。与前端 DiffEditor.charDiffSegs 的
    // MAX_CHAR_DIFF_LEN 保持一致（改动需两端同步）。
    const MAX_CHAR_DIFF_LEN: usize = 2000;
    if old.chars().count() > MAX_CHAR_DIFF_LEN || new.chars().count() > MAX_CHAR_DIFF_LEN {
        return (
            vec![DiffSegment { text: old.to_string(), changed: false }],
            vec![DiffSegment { text: new.to_string(), changed: false }],
        );
    }

    let old_chars: Vec<char> = old.chars().collect();
    let new_chars: Vec<char> = new.chars().collect();
    let m = old_chars.len();
    let n = new_chars.len();

    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in 1..=m {
        for j in 1..=n {
            if old_chars[i - 1] == new_chars[j - 1] {
                dp[i][j] = dp[i - 1][j - 1] + 1;
            } else {
                dp[i][j] = dp[i - 1][j].max(dp[i][j - 1]);
            }
        }
    }

    #[derive(PartialEq, Clone, Copy)]
    enum Op {
        Match(char),
        Insert(char),
        Delete(char),
    }

    let mut ops: Vec<Op> = Vec::new();
    let mut i = m;
    let mut j = n;
    while i > 0 || j > 0 {
        if i > 0 && j > 0 && old_chars[i - 1] == new_chars[j - 1] {
            ops.push(Op::Match(old_chars[i - 1]));
            i -= 1;
            j -= 1;
        } else if j > 0 && (i == 0 || dp[i][j - 1] >= dp[i - 1][j]) {
            ops.push(Op::Insert(new_chars[j - 1]));
            j -= 1;
        } else {
            ops.push(Op::Delete(old_chars[i - 1]));
            i -= 1;
        }
    }
    ops.reverse();

    // 旧侧分段：Match(不变) + Delete(变化)
    let mut old_segments: Vec<DiffSegment> = Vec::new();
    for op in &ops {
        let (text, changed) = match op {
            Op::Match(c) => (c.to_string(), false),
            Op::Delete(c) => (c.to_string(), true),
            Op::Insert(_) => continue,
        };
        push_segment(&mut old_segments, text, changed);
    }

    // 新侧分段：Match(不变) + Insert(变化)
    let mut new_segments: Vec<DiffSegment> = Vec::new();
    for op in &ops {
        let (text, changed) = match op {
            Op::Match(c) => (c.to_string(), false),
            Op::Insert(c) => (c.to_string(), true),
            Op::Delete(_) => continue,
        };
        push_segment(&mut new_segments, text, changed);
    }

    (old_segments, new_segments)
}

fn push_segment(segments: &mut Vec<DiffSegment>, text: String, changed: bool) {
    if let Some(last) = segments.last_mut() {
        if last.changed == changed {
            last.text.push_str(&text);
            return;
        }
    }
    segments.push(DiffSegment { text, changed });
}

/// 从 git tree 中读取指定路径文件内容（按行），并判定是否为二进制。
/// 返回 (行内容, 是否二进制)；文件不存在返回 (空, false)，二进制返回 (空, true)。
fn file_content_from_tree(
    repo: &Repository,
    tree: &git2::Tree,
    file_path: &str,
) -> (Vec<String>, bool) {
    match tree.get_path(Path::new(file_path)) {
        Ok(entry) => repo
            .find_blob(entry.id())
            .map(|blob| {
                // blob.is_binary() 用的是 git 自带启发式，与 is_binary_bytes 一致
                if blob.is_binary() {
                    return (Vec::new(), true);
                }
                (
                    String::from_utf8_lossy(blob.content())
                        .lines()
                        .map(|l| l.to_string())
                        .collect(),
                    false,
                )
            })
            .unwrap_or((Vec::new(), false)),
        Err(_) => (Vec::new(), false),
    }
}

#[command]
async fn get_file_diff(repo_path: String, file_path: String, commit_id: Option<String>) -> Result<FileDiff> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_read_guard();
        let repo = open_repo(&repo_path)?;

        let (old_content, new_content, is_binary) = if let Some(cid) = commit_id {
            // 提交历史模式：显示该提交相对父提交的修改对比
            let oid = git2::Oid::from_str(&cid).map_err(|e| e.to_string())?;
            let commit = repo.find_commit(oid).map_err(|e| e.to_string())?;
            let commit_tree = commit.tree().map_err(|e| e.to_string())?;
            let (new_content, new_bin) = file_content_from_tree(&repo, &commit_tree, &file_path);
            // 父提交中的文件内容（无父提交=新增文件，旧内容为空）
            let (old_content, old_bin) = if commit.parent_count() == 0 {
                (Vec::new(), false)
            } else {
                let parent = commit.parent(0).map_err(|e| e.to_string())?;
                let parent_tree = parent.tree().map_err(|e| e.to_string())?;
                file_content_from_tree(&repo, &parent_tree, &file_path)
            };
            (old_content, new_content, old_bin || new_bin)
        } else {
            // 工作区模式：工作区文件 vs HEAD
            let full_path = PathBuf::from(&repo_path).join(&file_path);
            let (new_content, new_bin) = read_file_lines(&full_path);
            let (old_content, old_bin) = {
                let head = repo.head().map_err(|e| e.to_string())?;
                let tree = head.peel_to_tree().map_err(|e| e.to_string())?;
                file_content_from_tree(&repo, &tree, &file_path)
            };
            (old_content, new_content, old_bin || new_bin)
        };

        // 大文件 / 二进制 跳过 diff 运算：
        // - 二进制：old/new 为空，diff 无意义
        // - 超大文本：Myers 是 O((N+M)·D) 线性空间，几万行也能算；
        //   仅当超过 10 万行（极端生成产物）才降级 oversized，前端仍可查看/编辑原文。
        const MAX_DIFF_LINES: usize = 100_000;
        let (old_content, new_content, lines, is_oversized) = if is_binary {
            (Vec::new(), Vec::new(), Vec::new(), false)
        } else if old_content.len() > MAX_DIFF_LINES || new_content.len() > MAX_DIFF_LINES {
            (old_content, new_content, Vec::new(), true)
        } else {
            let lines = lcs_diff(&old_content, &new_content);
            (old_content, new_content, lines, false)
        };

        // 归还仓库实例，供后续命令复用（避免重复 Repository::open 扫 .git）
        return_repo(&repo_path, repo);

        Ok(FileDiff {
            old_content,
            new_content,
            lines,
            is_binary,
            is_oversized,
        })
    })
    .await
    .map_err(|e| format!("加载文件差异失败: {}", e))?
}

/// 读取 git 暂存区指定 stage 的文件内容。
/// stage 1 = base, 2 = ours/HEAD, 3 = theirs/incoming
fn read_stage_content(repo_path: &str, stage: u8, file_path: &str) -> Option<Vec<String>> {
    let output = git_command()
        .arg("-C")
        .arg(repo_path)
        .arg("show")
        .arg(format!(":{}:{}", stage, file_path))
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let content = String::from_utf8_lossy(&output.stdout);
    Some(content.lines().map(|l| l.to_string()).collect())
}

/// 将工作区文件内容按行拆分，保留每行末尾的换行符，便于精确替换冲突块。
fn split_lines_keep_newline(content: &str) -> Vec<String> {
    let lines: Vec<&str> = content.split('\n').collect();
    lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            if i < lines.len() - 1 {
                format!("{}\n", line)
            } else {
                line.to_string()
            }
        })
        .collect()
}

/// 从工作区内容中解析标准 Git 冲突块（含可选的 diff3 base 段）。
fn parse_conflict_blocks(content: &str) -> Vec<ConflictBlock> {
    let lines = split_lines_keep_newline(content);
    let mut result = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        if lines[i].starts_with("<<<<<<< ") {
            let start_line = i;
            let marker_ours = lines[i][8..].trim_end().to_string();
            i += 1;

            let mut ours: Vec<String> = Vec::new();
            let mut base: Option<Vec<String>> = None;
            let mut theirs: Vec<String> = Vec::new();
            let mut separator_line = i;
            let mut section = 0; // 0 = ours, 1 = base (diff3), 2 = theirs

            while i < lines.len() {
                let cur = &lines[i];
                if cur.starts_with("||||||| ") {
                    base = Some(Vec::new());
                    section = 1;
                    i += 1;
                } else if cur.starts_with("=======") {
                    separator_line = i;
                    section = 2;
                    i += 1;
                } else if cur.starts_with(">>>>>>> ") {
                    let marker_theirs = cur[8..].trim_end().to_string();
                    result.push(ConflictBlock {
                        start_line,
                        separator_line,
                        end_line: i,
                        ours,
                        theirs,
                        base,
                        marker_ours,
                        marker_theirs,
                    });
                    i += 1;
                    break;
                } else {
                    match section {
                        0 => ours.push(cur.clone()),
                        1 => {
                            if let Some(ref mut b) = base {
                                b.push(cur.clone());
                            }
                        }
                        2 => theirs.push(cur.clone()),
                        _ => {}
                    }
                    i += 1;
                }
            }
        } else {
            i += 1;
        }
    }

    result
}

#[command]
async fn get_conflict_file(repo_path: String, file_path: String) -> Result<ConflictFile> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_read_guard();

        // ours = stage 2 (HEAD/current branch), theirs = stage 3 (incoming branch)
        let ours_content = read_stage_content(&repo_path, 2, &file_path).unwrap_or_default();
        let theirs_content = read_stage_content(&repo_path, 3, &file_path).unwrap_or_default();
        let base_content = read_stage_content(&repo_path, 1, &file_path);

        let full_path = PathBuf::from(&repo_path).join(&file_path);
        let working_raw = fs::read_to_string(&full_path).map_err(|e| e.to_string())?;
        let working_content = working_raw.lines().map(|l| l.to_string()).collect();

        let blocks = parse_conflict_blocks(&working_raw);

        Ok(ConflictFile {
            path: file_path,
            ours_content,
            theirs_content,
            base_content,
            working_content,
            blocks,
        })
    })
    .await
    .map_err(|e| format!("加载冲突文件失败: {}", e))?
}

#[command]
async fn read_working_file(repo_path: String, file_path: String) -> Result<String> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_read_guard();
        let full_path = PathBuf::from(&repo_path).join(&file_path);
        fs::read_to_string(&full_path).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("读取文件失败: {}", e))?
}

#[command]
async fn write_file_content(repo_path: String, file_path: String, content: String) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_write_guard();
        let full_path = PathBuf::from(&repo_path).join(&file_path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut file = File::create(&full_path).map_err(|e| e.to_string())?;
        file.write_all(content.as_bytes()).map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|e| format!("写入文件失败: {}", e))?
}

// ===== 储藏（Stash）=====

#[derive(serde::Serialize)]
struct StashEntry {
    index: i64,
    branch: String,
    message: String,
    date: String,
    stash_ref: String,
}

#[tauri::command]
async fn stash_create(
    repo_path: String,
    message: String,
    include_untracked: bool,
    keep_index: bool,
) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_write_guard();

        let mut cmd = git_command();
        cmd.arg("-C").arg(&repo_path).arg("stash").arg("push");

        if keep_index {
            cmd.arg("--keep-index");
        }
        if include_untracked {
            cmd.arg("-u");
        }
        if !message.is_empty() {
            cmd.arg("-m").arg(&message);
        }

        let output = cmd.output()
            .map_err(|e| format!("无法执行 git stash 命令: {}", e))?;

        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let msg = format!("{}{}", stderr, stdout);
            Err(if msg.trim().is_empty() { "储藏失败".to_string() } else { msg })
        }
    })
    .await
    .map_err(|e| format!("储藏失败: {}", e))?
}

#[tauri::command]
async fn stash_list(repo_path: String) -> Result<Vec<StashEntry>> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_read_guard();

        // 使用 git stash list 输出，格式：stash@{index}: On branch-name: message
        let output = git_command()
            .arg("-C")
            .arg(&repo_path)
            .arg("stash")
            .arg("list")
            .arg("--date=local")
            .output()
            .map_err(|e| format!("无法执行 git stash list 命令: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(if stderr.trim().is_empty() { "获取储藏列表失败".to_string() } else { stderr.to_string() });
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut entries = Vec::new();

        for line in stdout.lines() {
            if line.is_empty() {
                continue;
            }
            // 格式: stash@{0}: On branch: message
            // 或: stash@{0}: WIP on branch: message
            let caps = regex_capture(line);
            if let Some((index_str, rest)) = caps {
                let index: i64 = index_str.parse().unwrap_or(0);
                // rest 形如 "On branch-name: message" 或 "WIP on branch-name: message"
                let (branch, message) = parse_stash_rest(rest);
                entries.push(StashEntry {
                    index,
                    branch,
                    message,
                    date: String::new(), // stash list 默认不含日期，前端按需再取
                    stash_ref: format!("stash@{{{}}}", index),
                });
            }
        }

        // 按 index 倒序（最新在前）
        entries.sort_by(|a, b| b.index.cmp(&a.index));
        Ok(entries)
    })
    .await
    .map_err(|e| format!("获取储藏列表失败: {}", e))?
}

// 解析 stash 列表行的 index 和剩余部分
fn regex_capture(line: &str) -> Option<(&str, &str)> {
    // stash@{0}: On branch-name: message
    // 找到 "stash@{N}: " 前缀
    let prefix = "stash@{";
    if let Some(start) = line.find(prefix) {
        let rest = &line[start + prefix.len()..];
        if let Some(colon) = rest.find("}: ") {
            let index_str = &rest[..colon];
            let after = &rest[colon + 2..];
            return Some((index_str, after));
        }
    }
    None
}

// 解析 "On branch-name: message" 或 "WIP on branch-name: message"
fn parse_stash_rest(rest: &str) -> (String, String) {
    // 尝试去掉 "On " 或 "WIP on " 前缀
    let without_prefix = if let Some(suffix) = rest.strip_prefix("WIP on ") {
        suffix
    } else if let Some(suffix) = rest.strip_prefix("On ") {
        suffix
    } else {
        rest
    };

    // 找到 ": " 分隔符（第一个冒号+空格）
    if let Some(sep) = without_prefix.find(": ") {
        let branch = without_prefix[..sep].to_string();
        let message = without_prefix[sep + 2..].to_string();
        (branch, message)
    } else {
        (without_prefix.to_string(), String::new())
    }
}

#[tauri::command]
async fn stash_apply(repo_path: String, stash_ref: String, keep_index: bool) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_write_guard();

        let mut cmd = git_command();
        cmd.arg("-C").arg(&repo_path).arg("stash").arg("apply");

        if keep_index {
            cmd.arg("--index");
        }
        cmd.arg(&stash_ref);

        let output = cmd.output()
            .map_err(|e| format!("无法执行 git stash apply 命令: {}", e))?;

        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let msg = if stderr.trim().is_empty() { "应用储藏失败".to_string() } else { stderr.to_string() };
            Err(msg)
        }
    })
    .await
    .map_err(|e| format!("应用储藏失败: {}", e))?
}

#[tauri::command]
async fn stash_drop(repo_path: String, stash_ref: String) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_write_guard();

        let output = git_command()
            .arg("-C")
            .arg(&repo_path)
            .arg("stash")
            .arg("drop")
            .arg(&stash_ref)
            .output()
            .map_err(|e| format!("无法执行 git stash drop 命令: {}", e))?;

        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let msg = if stderr.trim().is_empty() { "删除储藏失败".to_string() } else { stderr.to_string() };
            Err(msg)
        }
    })
    .await
    .map_err(|e| format!("删除储藏失败: {}", e))?
}

// 在独立的新窗口中打开文件编辑/冲突解决器（不再用当前窗口的 Dialog 弹窗）。
// 传参方式：
// 1. URL hash 标记为 #/editor，让前端 main.ts 知道该挂载 EditorWindow。
// 2. 参数存入 EDIT_ARGS_MAP，前端 mount 后调用 get_edit_args(label) 主动获取。
// 3. 仍尝试 initialization_script 注入 window.__SNAPGIT_EDIT_ARGS__ 作为加速路径。
// 注意：WebviewUrl::App 不要带 search query，否则 dev 模式会触发
// "could not find webview label tauri.localhost" 并加载空白页。
#[command]
async fn open_edit_window(
    app: AppHandle,
    repo_path: String,
    file_path: String,
    mode: String,
    files_list: Option<Vec<String>>,
    current_index: Option<usize>,
) -> Result<()> {
    // 每次打开生成唯一窗口 label，避免覆盖之前的编辑窗口
    let label = format!(
        "editor-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    // 文件列表索引是前端数组下标；不在列表中（单文件打开）传 None，前端据此禁用导航按钮
    let current_index_value = current_index.filter(|&i| files_list.as_ref().is_some_and(|list| i < list.len()));
    let args = serde_json::json!({
        "repoPath": repo_path,
        "filePath": file_path,
        "mode": mode,
        "filesList": files_list,
        "currentIndex": current_index_value,
    });

    // 供前端 get_edit_args 读取
    {
        let mut map = EDIT_ARGS_MAP.lock().map_err(|e| e.to_string())?;
        map.insert(label.clone(), args.clone());
    }

    let script = format!("window.__SNAPGIT_EDIT_ARGS__ = {};", args);

    let _edit_window = {
        #[cfg(not(target_os = "macos"))]
        let builder = tauri::WebviewWindowBuilder::new(
            &app,
            &label,
            WebviewUrl::App("editor.html".into()),
        )
        .title("SnapGit - 文件编辑")
        .inner_size(1200.0, 800.0)
        .min_inner_size(700.0, 500.0)
        .resizable(true)
        // Windows/Linux: 关闭系统标题栏，由前端 TitleBar.vue 自绘（跟随主题切换）。
        // macOS: 保留系统标题栏（系统自动跟随 macOS 外观），让 traffic lights 显示正常。
        .decorations(false)
        .initialization_script(&script);

        #[cfg(target_os = "macos")]
        let builder = tauri::WebviewWindowBuilder::new(
            &app,
            &label,
            WebviewUrl::App("editor.html".into()),
        )
        .title("SnapGit - 文件编辑")
        .inner_size(1200.0, 800.0)
        .min_inner_size(700.0, 500.0)
        .resizable(true)
        .initialization_script(&script);

        builder
            .build()
            .map_err(|e| e.to_string())?
    };

    Ok(())
}

// 供独立编辑窗口前端获取打开时传入的参数。
// 支持三种查找方式（依次尝试）：
// 1. 前端传入显式 label（最可靠）
// 2. 用调用方 webview 的 label 自动匹配
// 3. 回退搜索：遍历 EDIT_ARGS_MAP 找第一个 editor-* 条目（兜底）
#[command]
fn get_edit_args(window: WebviewWindow, label: Option<String>) -> Result<serde_json::Value> {
    let map = EDIT_ARGS_MAP.lock().map_err(|e| e.to_string())?;

    // 方式 1：前端显式传入 label
    if let Some(ref explicit_label) = label {
        if let Some(args) = map.get(explicit_label) {
            return Ok(args.clone());
        }
    }

    // 方式 2：用 webview 自身 label
    let webview_label = window.label().to_string();
    if let Some(args) = map.get(&webview_label) {
        return Ok(args.clone());
    }

    // 方式 3：回退搜索 —— 找第一个 editor-* 条目
    if let Some((_, args)) = map.iter().find(|(k, _)| k.starts_with("editor-")) {
        return Ok(args.clone());
    }

    Err(format!(
        "未找到编辑参数 (explicit_label={:?}, webview_label={})",
        label, webview_label
    ))
}

// 关闭编辑窗口，并通知主窗口（main）刷新数据（保存文件后调用）。
#[command]
fn close_edit_window(app: AppHandle, window: Window) -> Result<()> {
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.emit("file-edited", ());
    }
    window.close().map_err(|e| e.to_string())?;
    Ok(())
}

#[command]
fn get_commit_files(repo_path: String, commit_id: String) -> Result<Vec<FileStatus>> {
    let _guard = git_read_guard();
    let repo = Repository::open(&repo_path).map_err(|e| e.to_string())?;

    let oid = git2::Oid::from_str(&commit_id).map_err(|e| e.to_string())?;
    let commit = repo.find_commit(oid).map_err(|e| e.to_string())?;
    let tree = commit.tree().map_err(|e| e.to_string())?;

    let parents: Vec<git2::Tree> = commit.parents()
        .filter_map(|p| p.tree().ok())
        .collect();

    let mut file_statuses = Vec::new();

    if parents.is_empty() {
        // 初始提交：所有文件都视为新增
        tree.walk(git2::TreeWalkMode::PreOrder, |_, entry| {
            if let Some(name) = entry.name() {
                if entry.kind() == Some(git2::ObjectType::Blob) {
                    file_statuses.push(FileStatus {
                        path: name.to_string(),
                        status: "new".to_string(),
                    });
                }
            }
            git2::TreeWalkResult::Ok
        }).map_err(|e| e.to_string())?;
    } else {
        // 与第一个父提交比较
        let parent_tree = &parents[0];
        let mut diff_opts = DiffOptions::new();
        diff_opts.include_untracked(true);

        let diff = repo.diff_tree_to_tree(
            Some(parent_tree),
            Some(&tree),
            Some(&mut diff_opts),
        ).map_err(|e| e.to_string())?;

        for delta in diff.deltas() {
            let path = delta.new_file().path()
                .or_else(|| delta.old_file().path())
                .and_then(|p| p.to_str())
                .unwrap_or("")
                .to_string();

            let status_str = match delta.status() {
                git2::Delta::Added => "new",
                git2::Delta::Modified => "modified",
                git2::Delta::Deleted => "deleted",
                git2::Delta::Renamed => "renamed",
                git2::Delta::Typechange => "typechange",
                _ => "unknown",
            };

            file_statuses.push(FileStatus {
                path,
                status: status_str.to_string(),
            });
        }
    }

    Ok(file_statuses)
}

// ===== 原生菜单栏（仅 macOS 使用，Windows/Linux 使用前端 Vue 组件 MenuBar.vue） =====

#[cfg(target_os = "macos")]
fn build_menu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    // 菜单项（触发前端事件）
    let open_repo_item = MenuItem::with_id(app, "open-repo", "打开仓库", true, None::<&str>)?;
    let clone_repo_item = MenuItem::with_id(app, "clone-repo", "克隆仓库", true, None::<&str>)?;
    let quit_item = PredefinedMenuItem::quit(app, Some("退出"))?;

    let commit_item = MenuItem::with_id(app, "commit", "提交", true, Some("CmdOrCtrl+S"))?;
    let push_item = MenuItem::with_id(app, "push", "推送", true, Some("CmdOrCtrl+Shift+P"))?;
    let pull_item = MenuItem::with_id(app, "pull", "拉取", true, Some("CmdOrCtrl+Shift+L"))?;
    let refresh_item = MenuItem::with_id(app, "refresh", "刷新", true, Some("CmdOrCtrl+R"))?;

    let new_branch_item = MenuItem::with_id(app, "new-branch", "新建分支", true, None::<&str>)?;
    let checkout_branch_item = MenuItem::with_id(app, "checkout-branch", "切换分支", true, None::<&str>)?;
    let merge_item = MenuItem::with_id(app, "merge", "合并分支", true, None::<&str>)?;

    let show_wt_item = MenuItem::with_id(app, "show-working-tree", "工作区", true, None::<&str>)?;
    let show_log_item = MenuItem::with_id(app, "show-log", "提交日志", true, None::<&str>)?;
    let toggle_theme_item = CheckMenuItem::with_id(app, "toggle-theme", "深色主题", true, true, None::<&str>)?;

    // 仓库配置（打开仓库配置对话框）
    let repo_config_item = MenuItem::with_id(app, "repo-config", "配置", true, None::<&str>)?;

    // 文件菜单
    let file_menu = Submenu::with_items(
        app,
        "文件",
        true,
        &[
            &open_repo_item,
            &clone_repo_item,
            &PredefinedMenuItem::separator(app)?,
            &quit_item,
        ],
    )?;

    // 编辑菜单
    let edit_menu = Submenu::with_items(
        app,
        "编辑",
        true,
        &[
            &repo_config_item,
        ],
    )?;

    // 仓库菜单
    let repo_menu = Submenu::with_items(
        app,
        "仓库",
        true,
        &[
            &commit_item,
            &push_item,
            &pull_item,
            &PredefinedMenuItem::separator(app)?,
            &refresh_item,
        ],
    )?;

    // 分支菜单
    let branch_menu = Submenu::with_items(
        app,
        "分支",
        true,
        &[
            &new_branch_item,
            &checkout_branch_item,
        ],
    )?;

    // 视图菜单
    let view_menu = Submenu::with_items(
        app,
        "视图",
        true,
        &[
            &PredefinedMenuItem::separator(app)?,
            &toggle_theme_item,
        ],
    )?;

    // 帮助菜单
    let help_menu = Submenu::with_items(
        app,
        "帮助",
        true,
        &[
            &MenuItem::with_id(app, "about", "关于 SnapGit", true, None::<&str>)?,
        ],
    )?;

    // 顶级菜单：macOS 会自动插入应用菜单（含 About、Services、Hide、Quit）
    // 我们这里依然按"文件"、"编辑"等配置，Windows 直接显示为窗口菜单
    let menu = Menu::with_items(
        app,
        &[&file_menu, &edit_menu, &repo_menu, &branch_menu, &view_menu, &help_menu],
    )?;

    Ok(menu)
}

// 将菜单事件转发给前端窗口（仅 macOS 使用）
#[cfg(target_os = "macos")]
fn on_menu_event<R: Runtime>(_app: &AppHandle<R>, window: &WebviewWindow<R>, id: &str) {
    let payload = serde_json::json!({ "action": id });
    let _ = window.emit("menu-action", payload);
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            // macOS: 使用系统原生菜单栏（显示在屏幕顶部）
            // Windows/Linux: 不设置原生菜单，由前端 Vue 组件 MenuBar.vue 渲染窗口内菜单栏
            #[cfg(target_os = "macos")]
            {
                // 构建原生菜单栏
                let menu = build_menu(&app.handle())?;
                app.set_menu(menu)?;
                let app_handle = app.handle().clone();
                app.on_menu_event(move |app, event| {
                    if let Some(win) = app.get_webview_window("main") {
                        on_menu_event(&app_handle, &win, event.id().as_ref());
                    }
                });
            }

            // Windows/Linux: 默认无原生窗口菜单，前端 MenuBar.vue 负责渲染
            #[cfg(not(target_os = "macos"))]
            {
                let _ = app.get_webview_window("main");
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            // 拦截主窗口关闭：统一交给前端决定——有 Git 操作在跑则弹确认框，
            // 空闲则前端调 destroy() 直接退出。防止前端监听失效导致关不掉，
            // 只拦 "main"，编辑窗口关闭不受影响。
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.emit("close-requested", ());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            is_git_busy,
            open_repository,
            init_repository,
            get_commits,
            get_branches,
            get_remotes,
            get_upstream,
            get_file_status,
            stage_file,
            discard_file,
            commit,
            push,
            get_remote_url,
            save_credentials,
            get_repo_config,
            checkout_branch,
            checkout_remote_branch,
            pull_branch,
            clone_repository,
            add_remote,
            create_branch,
            get_current_branch,
            merge_branch,
            rename_branch,
            delete_branch,
            delete_remote_branch,
            check_remote_branch_deletable,
            open_folder_dialog,
            open_file_dialog,
            set_ssh_key_path,
            detect_git,
            load_recent_repositories,
            save_recent_repository,
            remove_recent_repository,
            scan_projects,
            get_file_tree,
            get_file_diff,
            get_commit_files,
            get_conflict_file,
            read_working_file,
            write_file_content,
            open_edit_window,
            close_edit_window,
            get_edit_args,
            stash_create,
            stash_list,
            stash_apply,
            stash_drop,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod diff_tests {
    use super::*;

    /// 旧版 O(m*n) LCS 差分（保留副本作为基准，仅测试对照用）：
    /// 返回 (line_type, content) 序列，语义与生产 lcs_diff 完全一致。
    fn legacy_lcs_pairs(old: &[String], new: &[String]) -> Vec<(String, String)> {
        let m = old.len();
        let n = new.len();
        let mut dp = vec![vec![0usize; n + 1]; m + 1];
        for i in 1..=m {
            for j in 1..=n {
                if old[i - 1] == new[j - 1] {
                    dp[i][j] = dp[i - 1][j - 1] + 1;
                } else {
                    dp[i][j] = dp[i - 1][j].max(dp[i][j - 1]);
                }
            }
        }
        let mut pairs: Vec<(String, String)> = Vec::new();
        let mut i = m;
        let mut j = n;
        while i > 0 || j > 0 {
            if i > 0 && j > 0 && old[i - 1] == new[j - 1] {
                pairs.push(("context".to_string(), old[i - 1].clone()));
                i -= 1;
                j -= 1;
            } else if j > 0 && (i == 0 || dp[i][j - 1] >= dp[i - 1][j]) {
                pairs.push(("add".to_string(), new[j - 1].clone()));
                j -= 1;
            } else if i > 0 {
                pairs.push(("delete".to_string(), old[i - 1].clone()));
                i -= 1;
            }
        }
        pairs.reverse();
        // 与生产一致：相邻 delete+add 合并为 modified
        let mut merged: Vec<(String, String)> = Vec::new();
        let mut k = 0;
        while k < pairs.len() {
            if k + 1 < pairs.len() && pairs[k].0 == "delete" && pairs[k + 1].0 == "add" {
                merged.push(("modified".to_string(), pairs[k].1.clone()));
                k += 2;
            } else {
                merged.push(pairs[k].clone());
                k += 1;
            }
        }
        merged
    }

    fn to_lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// 对照测试核心：Myers 产出与旧 LCS 基准一致（合并 modified 后逐行比较 type/content）
    fn assert_myers_matches(old: &[&str], new: &[&str]) {
        let old_l = to_lines(old);
        let new_l = to_lines(new);
        let legacy = legacy_lcs_pairs(&old_l, &new_l);
        let actual: Vec<(String, String)> = lcs_diff(&old_l, &new_l)
            .into_iter()
            .map(|l| (l.line_type, l.content))
            .collect();
        assert_eq!(
            actual, legacy,
            "Myers diff mismatch\nold={old:?}\nnew={new:?}"
        );
    }

    #[test]
    fn myers_empty_both() {
        let r = lcs_diff(&[], &[]);
        assert!(r.is_empty());
    }

    #[test]
    fn myers_identical() {
        assert_myers_matches(
            &["a", "b", "c"],
            &["a", "b", "c"],
        );
    }

    #[test]
    fn myers_all_new() {
        assert_myers_matches(&[], &["x", "y"]);
    }

    #[test]
    fn myers_all_deleted() {
        assert_myers_matches(&["x", "y"], &[]);
    }

    #[test]
    fn myers_insert_middle() {
        assert_myers_matches(&["a", "b"], &["a", "ins", "b"]);
    }

    #[test]
    fn myers_delete_middle() {
        assert_myers_matches(&["a", "gone", "b"], &["a", "b"]);
    }

    #[test]
    fn myers_modify_line() {
        assert_myers_matches(&["a", "old line", "c"], &["a", "new line", "c"]);
    }

    #[test]
    fn myers_complex_edits() {
        assert_myers_matches(
            &[
                "keep1", "keep2", "del_a", "del_b", "keep3", "change_me", "keep4", "tail1",
            ],
            &[
                "keep1", "keep2", "keep3", "changed!", "keep4", "extra_new", "tail1", "tail2",
            ],
        );
    }

    #[test]
    fn myers_reversed_words() {
        // 完全重排（D 接近 N+M，验证最坏情况不会挂、结果仍是合法 diff）
        let old = to_lines(&["one", "two", "three", "four", "five"]);
        let new = to_lines(&["five", "four", "three", "two", "one"]);
        let r = lcs_diff(&old, &new);
        // 合法 diff 的可达性验证：从产出序列能重建 new
        let mut rebuilt: Vec<String> = Vec::new();
        for l in &r {
            match l.line_type.as_str() {
                "context" | "modified" | "add" => rebuilt.push(l.content.clone()),
                "delete" => {}
                _ => {}
            }
        }
        assert_eq!(rebuilt, new, "rebuilt new content mismatch");
        // 且 old 侧行号覆盖完整
        let mut covered = vec![false; old.len()];
        for l in &r {
            if let Some(n) = l.old_line {
                covered[n - 1] = true;
            }
        }
        assert!(
            covered.iter().all(|&c| c),
            "some old lines not covered: {covered:?}"
        );
    }

    #[test]
    fn myers_line_numbers_monotonic() {
        // 行号语义验证：context 的 old_line/new_line 递增、delete 行号递增、add 行号递增
        let old = to_lines(&["a", "b", "c", "d", "e"]);
        let new = to_lines(&["a", "x", "c", "e", "f"]);
        let r = lcs_diff(&old, &new);
        let mut last_old = 0usize;
        let mut last_new = 0usize;
        for l in &r {
            if let Some(o) = l.old_line {
                assert!(o > last_old, "old_line not increasing: {o} after {last_old}");
                last_old = o;
            }
            if let Some(n) = l.new_line {
                assert!(n > last_new, "new_line not increasing: {n} after {last_new}");
                last_new = n;
            }
        }
    }

    #[test]
    fn myers_large_file_smoke() {
        // 超过旧 5000 行上限的大文件冒烟：Myers 能秒算、不 OOM
        let n = 20_000usize;
        let old: Vec<String> = (0..n).map(|i| format!("line {i:06}").to_string()).collect();
        let mut new = old.clone();
        // 改动若干行（D 小，Myers 应极快）
        for i in [3usize, 1000, 9999, 19999] {
            new[i] = format!("line {i:06} CHANGED").to_string();
        }
        // 额外加 5 行
        for i in 0..5usize {
            new.push(format!("extra tail {i}").to_string());
        }
        let t = std::time::Instant::now();
        let r = lcs_diff(&old, &new);
        assert!(r.len() > n, "expected diff covering all lines, got {}", r.len());
        // 行号覆盖验证
        let mut covered_new = vec![false; new.len()];
        for l in &r {
            if let Some(no) = l.new_line {
                covered_new[no - 1] = true;
            }
        }
        assert!(covered_new.iter().all(|&c| c), "new line coverage broken");
        eprintln!("myers_large_file_smoke took {:?} for {n} lines", t.elapsed());
    }

    #[test]
    fn diff_chars_short_line_marks_changes() {
        // 正常长度行：字符级 diff 应标出 changed 段
        let (old_segs, new_segs) = diff_chars("abc def ghi", "abc XYZ ghi");
        assert!(!old_segs.is_empty() && !new_segs.is_empty());
        assert!(
            old_segs.iter().any(|s| s.changed) && new_segs.iter().any(|s| s.changed),
            "short-line diff should flag changed chars"
        );
        // 两行内容可完整重建
        let old_joined: String = old_segs.iter().map(|s| s.text.as_str()).collect();
        let new_joined: String = new_segs.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(old_joined, "abc def ghi");
        assert_eq!(new_joined, "abc XYZ ghi");
    }

    #[test]
    fn diff_chars_long_line_degrades_gracefully() {
        // 超长行（>2000 字符，如压缩产物单行）：不 panic、不 OOM，整行降级为单段 changed=false
        let long_a = "x".repeat(2500);
        let long_b = "y".repeat(2500);
        let (old_segs, new_segs) = diff_chars(&long_a, &long_b);
        assert_eq!(old_segs.len(), 1, "oversized old should collapse to 1 segment");
        assert_eq!(new_segs.len(), 1, "oversized new should collapse to 1 segment");
        assert!(!old_segs[0].changed && !new_segs[0].changed, "oversized line should NOT be flagged as fully changed");
        assert_eq!(old_segs[0].text.len(), 2500);
        assert_eq!(new_segs[0].text.len(), 2500);

        // 一侧超长也应触发退化
        let (o2, n2) = diff_chars(&"short".to_string(), &long_b);
        assert_eq!(n2.len(), 1);
        assert_eq!(o2[0].text, "short");
    }
}