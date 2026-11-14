use super::*;


// 读写锁：写命令（commit/checkout/push/stage 等）独占（与读、写都互斥），只读命令
// （get_commits/get_branches/get_file_status/stash_list 等）可并发共享。既保留原保护语义
// —— 防止并发 checkout（写）与其他读命令交错损坏 .git 对象库、进程崩溃（闪退），
// 又让 loadRepoData 的多个只读命令真正并行，不再因全局串行而排队（切换仓库卡顿的根因）。
pub(crate) static GIT_LOCK: RwLock<()> = RwLock::new(());

/// 正在执行中的 Git 操作计数：guard 获取时 +1，Drop 时 -1。
/// 关闭主窗口时据此判断「是否有 Git 进程/操作仍在跑」，忙则弹确认框。
pub(crate) static ACTIVE_GIT_OPS: AtomicUsize = AtomicUsize::new(0);

/// 只读 guard：持有 GIT_LOCK 读锁并计入 ACTIVE_GIT_OPS。
pub(crate) struct GitReadGuard(#[allow(dead_code)] std::sync::RwLockReadGuard<'static, ()>);
impl Drop for GitReadGuard {
    fn drop(&mut self) {
        ACTIVE_GIT_OPS.fetch_sub(1, Ordering::SeqCst);
    }
}
pub(crate) fn git_read_guard() -> GitReadGuard {
    let g = GIT_LOCK.read().unwrap_or_else(|e| e.into_inner());
    ACTIVE_GIT_OPS.fetch_add(1, Ordering::SeqCst);
    GitReadGuard(g)
}

/// 写 guard：持有 GIT_LOCK 写锁并计入 ACTIVE_GIT_OPS。
pub(crate) struct GitWriteGuard(#[allow(dead_code)] std::sync::RwLockWriteGuard<'static, ()>);
impl Drop for GitWriteGuard {
    fn drop(&mut self) {
        ACTIVE_GIT_OPS.fetch_sub(1, Ordering::SeqCst);
    }
}
pub(crate) fn git_write_guard() -> GitWriteGuard {
    let g = GIT_LOCK.write().unwrap_or_else(|e| e.into_inner());
    ACTIVE_GIT_OPS.fetch_add(1, Ordering::SeqCst);
    GitWriteGuard(g)
}

// 文件状态缓存：自动刷新高频调用 get_file_status 时命中，避免每次都扫 .git。
// TTL 短（800ms）以控制 stale；loadRepoData 等主动刷新走 cached=false 绕过，
// 保证切库 / 写操作后即时准确。仅缓存 get_file_status（autoRefresh 高频路径）。
pub(crate) static STATUS_CACHE: LazyLock<Mutex<HashMap<String, (Instant, Vec<FileStatus>)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
pub(crate) const STATUS_CACHE_TTL: Duration = Duration::from_millis(800);

/// 仓库实例缓存：避免每次命令都 `Repository::open` 重扫 .git（大仓库每次几十 ms）。
/// 通过 remove/insert 转移所有权，保证同一时刻只有一个任务持有某仓库实例
/// （git2::Repository 不是 Sync，但可在任务间 Send 移动，无需共享借用）。
pub(crate) static REPO_CACHE: LazyLock<Mutex<HashMap<String, git2::Repository>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
pub(crate) const REPO_CACHE_LIMIT: usize = 8;

/// 取仓库实例：优先复用缓存（remove 出所有权），否则新建。错误已 map 成 String 以便 `?` 传播。
pub(crate) fn open_repo(repo_path: &str) -> Result<git2::Repository> {
    if let Some(repo) = REPO_CACHE.lock().unwrap().remove(repo_path) {
        return Ok(repo);
    }
    git2::Repository::open(repo_path).map_err(|e| e.to_string())
}

/// 查询当前是否有 Git 操作正在执行（供关闭窗口前的确认弹窗使用）。
#[command]
pub(crate) fn is_git_busy() -> bool {
    ACTIVE_GIT_OPS.load(Ordering::SeqCst) > 0
}

/// 归还仓库实例到缓存（调用方不再持有）。超过上限则整体清空，避免多仓库无限累积。
pub(crate) fn return_repo(repo_path: &str, repo: git2::Repository) {
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
pub(crate) fn git_command() -> std::process::Command {
    // creation_flags 仅在 Windows 编译；非 Windows 上 cmd 不会被可变借用，
    // 用 cfg_attr 只在非 Windows 允许 unused_mut，避免 Windows 编译时缺 mut。
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut cmd = std::process::Command::new("git");
    #[cfg(windows)]
    cmd.creation_flags(0x08000000);
    cmd
}

/// 解析单条 git 进度片段，提取 (阶段键, 该阶段内的百分比)。非进度行返回 None。
///
/// 进度片段形如 `remote: Receiving objects:  42% (100/238), 1.2 MiB | 300 KiB/s`。
///
/// 【重要】git 的进度是用 `\r`（回车）覆盖**同一行**来刷新的，一个阶段会连续刷新
/// 上百次。因此调用方必须先按 `\r` 与 `\n` 双重切分，否则整个阶段的上百次刷新会被
/// 当成一行读入，解析时只取到片段里最后一个 `NN%`（恒为 100%），
/// 造成「还在压缩对象就显示 100%」的错觉。
pub(crate) fn parse_git_progress(line: &str) -> Option<(&'static str, u32)> {
    let l = line.trim();
    let phase = if l.contains("Receiving objects") {
        "receiving"
    } else if l.contains("Resolving deltas") {
        "resolving"
    } else if l.contains("Counting objects") {
        "counting"
    } else if l.contains("Compressing objects") {
        "compressing"
    } else {
        return None;
    };
    // 提取片段中最后一个 "NN%" 前的数字作为该阶段百分比
    let bytes = l.as_bytes();
    let mut pct: Option<u32> = None;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let mut j = i;
            while j > 0 && bytes[j - 1].is_ascii_digit() {
                j -= 1;
            }
            if j < i {
                if let Ok(n) = l[j..i].parse::<u32>() {
                    pct = Some(n);
                }
            }
        }
        i += 1;
    }
    let pct = pct?;
    Some((phase, pct))
}

/// 把「阶段 + 阶段内百分比」换算成**整体拉取进度**（0-100）。
///
/// 必要性：git 各阶段的百分比是各自独立从 0 重新计的。若直接把阶段百分比透给前端，
/// 会出现「计数 100% → 压缩 0% → 接收 0%」的来回跳变与「尚未拉取完就显示 100%」。
/// 这里给每个阶段分配一个互不重叠的进度区间，使整体进度单调递增，
/// 并且除真正完成（成功回调）外一律封顶 99%，100% 只在确实拉取完成时出现。
pub(crate) fn overall_progress(phase: &str, phase_pct: u32) -> u32 {
    let (start, end) = match phase {
        "counting" => (0u32, 8u32),
        "compressing" => (8, 20),
        "receiving" => (20, 80),
        "resolving" => (80, 99),
        _ => (0, 99),
    };
    let p = phase_pct.min(100);
    (start + (end - start) * p / 100).min(99)
}

/// 处理一条 git stderr 片段：能解析成进度就换算整体进度并推送事件（只增不减，避免闪回），
/// 否则原样收集到 `captured`（用于失败时的错误提示）。
/// 纯进度片段不进 `captured`，避免失败提示里塞满成百上千行百分比。
pub(crate) fn handle_progress_segment(
    seg: &str,
    app: &AppHandle,
    captured: &mut String,
    last_pct: &mut u32,
) {
    let t = seg.trim();
    if t.is_empty() {
        return;
    }
    if let Some((phase, phase_pct)) = parse_git_progress(t) {
        let overall = overall_progress(phase, phase_pct);
        let increased = overall > *last_pct;
        if increased {
            *last_pct = overall;
        }
        // receiving（真正下载对象）阶段**每个片段都发**：前端统计行的大小/速度依赖
        // detail 实时刷新，只按整体百分比节流会让统计卡在旧值甚至错过不显示
        // （古哥 2026-10-21）。其余阶段数量已够用且不带大小，仍按百分比节流避免事件洪水。
        if increased || phase == "receiving" {
            let _ = app.emit(
                "pull-progress",
                serde_json::json!({
                    "percent": *last_pct,
                    "phase": phase,
                    "detail": t,
                }),
            );
        }
        return;
    }
    captured.push_str(t);
    captured.push('\n');
}

/// 流式执行 git 子命令，把 fetch/pull 的实时进度通过 `pull-progress` 事件推送给前端。
/// 管道下 git 默认不输出进度，因此 fetch/pull 强制加 `--progress`。返回 (是否成功, 合并后的输出)。
pub(crate) fn run_git_progress(
    app: &AppHandle,
    repo_path: &str,
    subcmd: &str,
    extra_args: &[&str],
    ssh_cmd: Option<&str>,
) -> std::io::Result<(bool, String)> {
    let mut cmd = git_command();
    cmd.arg("-C").arg(repo_path).arg(subcmd);
    for a in extra_args {
        cmd.arg(a);
    }
    // 网络/合并类子命令在管道下默认不输出进度，强制输出以便解析百分比
    if subcmd == "fetch" || subcmd == "pull" {
        cmd.arg("--progress");
    }
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    // 自动检测到的 SSH 私钥：通过 GIT_SSH_COMMAND 仅本次生效（优先级高于 core.sshCommand），
    // 拉取成功后再由调用方持久化进仓库 config。
    if let Some(s) = ssh_cmd {
        cmd.env("GIT_SSH_COMMAND", s);
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd.spawn()?;

    // stderr：进度信息 + 报错都走这里。
    // 【关键】git 用 `\r` 回车覆盖同一行来刷新进度（一个阶段可连续刷新上百次），
    // `BufReader::lines()` 只按 `\n` 切分，会把整个阶段的上百次刷新读成「一行」，
    // 导致解析时只取到最后一个 NN%（恒为 100%）→ 表现为「还没拉完就显示 100%」。
    // 因此这里手工读取字节流，并同时以 `\r` 与 `\n` 作为分隔符逐片段处理。
    let stderr = child.stderr.take().expect("stderr 应当可获取");
    let app_stderr = app.clone();
    let stderr_thread = std::thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        let mut captured = String::new();
        let mut last_pct: u32 = 0;
        let mut chunk = [0u8; 4096];
        let mut pending: Vec<u8> = Vec::new();
        loop {
            let n = match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => n,
                Err(_) => break,
            };
            pending.extend_from_slice(&chunk[..n]);
            // 取出所有完整片段（以 \r 或 \n 结尾），保留末尾残片到下一轮
            let mut start = 0usize;
            for i in 0..pending.len() {
                if pending[i] == b'\r' || pending[i] == b'\n' {
                    if let Ok(seg) = std::str::from_utf8(&pending[start..i]) {
                        handle_progress_segment(seg, &app_stderr, &mut captured, &mut last_pct);
                    }
                    start = i + 1;
                }
            }
            pending.drain(..start);
        }
        // 流结束时处理末尾残片
        if !pending.is_empty() {
            if let Ok(seg) = std::str::from_utf8(&pending) {
                handle_progress_segment(seg, &app_stderr, &mut captured, &mut last_pct);
            }
        }
        captured
    });

    // stdout：成功信息（如 "Already up to date."）
    let mut stdout = child.stdout.take().expect("stdout 应当可获取");
    let stdout_thread = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout.read_to_string(&mut s);
        s
    });

    let status = child.wait()?;
    // 注意：这里不发 phase:"done" 事件——一次拉取 = fetch+pull 两条命令，每条成功都发
    // 会造成「拉取完成」在变更文件面板闪现多次，且 fetch 的 done 会把进度面板中途藏掉。
    // 完成态由前端 finishPullProgress 统一设置（唯一来源）。
    let stderr_out = stderr_thread.join().unwrap_or_default();
    let stdout_out = stdout_thread.join().unwrap_or_default();
    let combined = format!("{}{}", stderr_out, stdout_out);
    Ok((status.success(), combined))
}

// 编辑窗口参数仓库：key = 窗口 label，value = { repoPath, filePath, mode }。
// 独立 webview 的 initialization_script 注入不可靠，改为前端 mount 后主动来取。
pub(crate) static EDIT_ARGS_MAP: std::sync::LazyLock<Mutex<std::collections::HashMap<String, serde_json::Value>>> =
    std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));
