use super::*;


#[command]
pub(crate) async fn checkout_branch(repo_path: String, branch_name: String, force: Option<bool>) -> Result<()> {
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
pub(crate) fn checkout_remote_branch(repo_path: String, remote_branch: String) -> Result<()> {
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

/// 解析 `git pull` 的目标参数：
/// - 当前分支已有 upstream → 返回空，走裸 `git pull`（沿用用户既有跟踪配置）；
/// - 没有 upstream（典型：首次 push 未带 -u）→ 返回 `<remote> <branch>` 显式拉取，
///   避免 git 报 "There is no tracking information for the current branch"。
///   注意：不写用户仓库配置（不自动 set-upstream），保持无副作用。
/// - HEAD 游离 / 仓库打不开 / 无 remote → 返回空，保持原有报错行为。
pub(crate) fn resolve_pull_target(repo_path: &str) -> Vec<String> {
    let Ok(repo) = Repository::open(repo_path) else {
        return Vec::new();
    };
    // 当前分支名：
    //  - 正常 HEAD（有提交）→ shorthand；
    //  - 空仓库（刚 git init 未提交，HEAD 为 unborn）→ head() 会失败，
    //    改从 HEAD 的 symbolic ref（refs/heads/<name>）取分支名；仍拿不到则默认 main。
    //  （此前空仓库直接返回空 → 退回裸 `git pull` → 报 "There is no tracking information"）
    let branch_name = repo
        .head()
        .ok()
        .and_then(|h| h.shorthand().map(|s| s.to_string()))
        .or_else(|| {
            repo.find_reference("HEAD")
                .ok()
                .and_then(|r| r.symbolic_target().map(|s| s.to_string()))
                .map(|target| {
                    target
                        .strip_prefix("refs/heads/")
                        .unwrap_or(&target)
                        .to_string()
                })
        })
        .unwrap_or_else(|| "main".to_string());
    // 已有 upstream → 裸 pull 即可
    if let Ok(local) = repo.find_branch(&branch_name, BranchType::Local) {
        if local.upstream().is_ok() {
            return Vec::new();
        }
    }
    // 没有 upstream：选 remote（优先 origin，否则取第一个）
    let remotes = match repo.remotes() {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    let remote = remotes
        .iter()
        .flatten()
        .find(|r| *r == "origin")
        .or_else(|| remotes.iter().flatten().next());
    match remote {
        Some(r) => vec![r.to_string(), branch_name],
        None => Vec::new(),
    }
}

/// 工作区是否包含未提交改动（含未跟踪文件）。用于判断拉取前是否需要先 stash。
pub(crate) fn is_worktree_dirty(repo_path: &str) -> bool {
    let out = git_command()
        .arg("-C")
        .arg(repo_path)
        .arg("status")
        .arg("--porcelain")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output();
    match out {
        Ok(o) => !String::from_utf8_lossy(&o.stdout).trim().is_empty(),
        Err(_) => false,
    }
}

/// 拉取前自动 stash 未提交改动。原因：git 在脏工作区直接 pull 会直接 abort
/// （"Your local changes would be overwritten by merge"），无法产生真正的合并冲突。
/// 先 stash 让工作区干净，拉取成功后再 pop 还原；若与远端改动重叠则产生冲突，
/// 由「变更文件」面板标红展示（不弹窗、不阻塞）。仅脏工作区时执行。
/// 返回是否真的 stash 了（用于拉取后还原）。
pub(crate) fn auto_stash_before_pull(repo_path: &str) -> bool {
    if !is_worktree_dirty(repo_path) {
        return false;
    }
    let out = git_command()
        .arg("-C")
        .arg(repo_path)
        .args(["stash", "push", "-u", "-m", "snapgit-pull-autostash"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .output();
    matches!(out, Ok(o) if o.status.success())
}

/// 拉取后还原 stash（`git stash pop`）。
/// - 成功：返回 None（改动已还原）。
/// - 冲突：保留 stash 不丢弃，返回冲突信息，交由前端冲突流程处理。
pub(crate) fn auto_unstash_after_pull(repo_path: &str) -> Option<String> {
    let out = git_command()
        .arg("-C")
        .arg(repo_path)
        .args(["stash", "pop"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .output();
    match out {
        Ok(o) => {
            let combined = format!(
                "{}{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            );
            if !o.status.success() || combined.contains("CONFLICT") || combined.contains("could not apply") {
                Some(combined)
            } else {
                None
            }
        }
        Err(_) => None,
    }
}

/// 从 SSH 默认路径检测**所有**存在的私钥文件（跳过 .pub），按 id_ed25519 → id_rsa → id_ecdsa 顺序。
/// 用于「未配置 SSH 私钥路径时自动兜底」：拉取成功后由调用方持久化进仓库 config（仅恰好一把时）。
pub(crate) fn detect_default_ssh_keys() -> Vec<String> {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return Vec::new(),
    };
    ["id_ed25519", "id_rsa", "id_ecdsa"]
        .iter()
        .map(|name| home.join(".ssh").join(name))
        .filter(|p| p.is_file())
        .map(|p| p.to_string_lossy().to_string())
        .collect()
}

/// 仓库是否已配置 core.sshCommand（查所有层级：local/global）。
/// 已配置则不自动检测，尊重用户既有设置。
pub(crate) fn repo_has_ssh_command(repo_path: &str) -> bool {
    git_command()
        .arg("-C")
        .arg(repo_path)
        .arg("config")
        .arg("--get")
        .arg("core.sshCommand")
        .output()
        .map(|o| o.status.success() && !String::from_utf8_lossy(&o.stdout).trim().is_empty())
        .unwrap_or(false)
}

/// 把 SSH 私钥路径写入仓库本地 config（core.sshCommand）。
/// 刻意不加 git 锁——供已持锁的调用方（set_ssh_key_path 持读锁、拉取流程持写锁）安全复用。
pub(crate) fn apply_ssh_key_config(repo_path: &str, key_path: &str) -> Result<String> {
    // 规范化为正斜杠（Git 风格）：Windows 反斜杠路径在 core.sshCommand 经 git 的
    // split_cmdline / sh 解析时属于脆弱写法，部分 git/ssh 版本或环境下会被当作转义符吃错，
    // 导致私钥路径解析失败而拉取报认证错误。正斜杠在 Git for Windows 下始终可用。
    let key_path = key_path.trim().replace('\\', "/");
    if key_path.is_empty() {
        // 清空：移除 core.sshCommand（忽略失败，可能本来就没设）
        let _ = git_command()
            .arg("-C").arg(repo_path)
            .arg("config").arg("--local").arg("--unset")
            .arg("core.sshCommand")
            .output();
        return Ok(String::new());
    }
    let cmd = format!("ssh -i \"{}\" -o IdentitiesOnly=yes", key_path);
    let status = git_command()
        .arg("-C").arg(repo_path)
        .arg("config").arg("--local")
        .arg("core.sshCommand").arg(&cmd)
        .status();
    match status {
        Ok(s) if s.success() => Ok(cmd),
        Ok(s) => Err(format!("git config 写入失败，退出码: {}", s)),
        Err(e) => Err(format!("执行 git 失败: {}", e)),
    }
}

/// 拉取前的 SSH 私钥自动兜底：未配置 core.sshCommand 且默认路径存在私钥时，
/// 返回 (本次用的 GIT_SSH_COMMAND, 可持久化的私钥路径 Option)。
/// 已配置或未找到私钥 → None（沿用 git 默认行为）。
///
/// 【为什么把所有检测到的私钥都列进命令】旧实现只取第一把（id_ed25519 优先）并
/// `IdentitiesOnly=yes` 屏蔽其余——用户 ~/.ssh 里同时存在已注册的 id_rsa 和未注册的
/// id_ed25519 时，兜底会钉死在错的 ed25519 上，拉取报 Permission denied (publickey)
/// （而 git 默认行为把默认身份全试一遍反而是通的，2026-10-21 实测）。
/// 现改为 `ssh -i key1 -i key2 ... -o IdentitiesOnly=yes`：只提供列出的身份、按顺序
/// 逐把尝试，服务器认哪把用哪把；既不掺 agent 里的杂 key（避免 Too many authentication
/// failures），也不会钉死在某一把上。
///
/// 持久化策略：恰好一把时写入仓库 config（无歧义）；多把时无法确定服务器认哪把，
/// 返回 None 不持久化，每次拉取动态生成。
pub(crate) fn auto_ssh_key(repo_path: &str) -> Option<(String, Option<String>)> {
    if repo_has_ssh_command(repo_path) {
        return None;
    }
    let keys = detect_default_ssh_keys();
    if keys.is_empty() {
        return None;
    }
    let identities = keys
        .iter()
        .map(|k| format!("-i \"{}\"", k.replace('\\', "/")))
        .collect::<Vec<_>>()
        .join(" ");
    let ssh_cmd = format!("ssh {identities} -o IdentitiesOnly=yes");
    let persist = if keys.len() == 1 { Some(keys[0].clone()) } else { None };
    Some((ssh_cmd, persist))
}

#[command]
pub(crate) async fn pull_branch(app: AppHandle, repo_path: String) -> Result<()> {
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

        // 拉取前若工作区有未提交改动，先自动 stash（git 在脏工作区直接 pull 会 abort，无法产生真正冲突）。
        // stash 后工作区干净可正常拉取，还原时若与远端改动重叠则产生冲突，由「变更文件」面板标红展示。
        let stashed = auto_stash_before_pull(&repo_path);

        // SSH 私钥自动兜底：未配置 core.sshCommand 时，从 SSH 默认路径检测存在的私钥，
        // 本次 fetch/pull 通过 GIT_SSH_COMMAND 使用；拉取成功后自动持久化进仓库 config。
        let auto_ssh = auto_ssh_key(&repo_path);
        let ssh_for_run = auto_ssh.as_ref().map(|(c, _)| c.as_str());

        // 先 fetch 远程最新代码（流式执行，实时推送拉取进度）
        let (ok, out) = run_git_progress(&app, &repo_path, "fetch", &["--all"], ssh_for_run)
            .map_err(|e| format!("无法执行 git fetch 命令: {}", e))?;
        if !ok {
            let mut msg = out;
            if msg.trim().is_empty() {
                msg = "获取远程代码失败".to_string();
            }
            if msg.contains("Authentication failed") || msg.contains("could not read Username") || msg.contains("terminal prompts disabled") {
                msg = format!("拉取失败：远程仓库认证失败\n\n请确保已配置认证方式（SSH 密钥或 credential helper）。\n\n原始错误:\n{}", msg);
            }
            // 拉取失败：还原 stash，避免改动被留在工作区之外
            if stashed {
                let _ = auto_unstash_after_pull(&repo_path);
            }
            return Err(msg);
        }

        // 分支没有 upstream 时显式指定 <remote> <branch>（如首次 push 未带 -u 的仓库），
        // 避免 git 报 "There is no tracking information for the current branch"
        let mut pull_args: Vec<String> = vec!["--ff-only".to_string()];
        pull_args.extend(resolve_pull_target(&repo_path));
        let pull_args: Vec<&str> = pull_args.iter().map(|s| s.as_str()).collect();

        // 再尝试 fast-forward merge（流式执行，实时推送拉取进度）
        let (ok2, out2) = run_git_progress(&app, &repo_path, "pull", &pull_args, ssh_for_run)
            .map_err(|e| format!("无法执行 git pull 命令: {}", e))?;
        if ok2 {
            // 拉取成功：把自动检测到的 SSH 私钥路径持久化进仓库 config（仅恰好检测到一把时，
            // 多把时不写，见 auto_ssh_key 注释），后续 fetch/pull/push 复用（失败仅忽略）。
            if let Some((_, Some(key))) = &auto_ssh {
                let _ = apply_ssh_key_config(&repo_path, key);
            }
            // 拉取成功：还原 stash。若与远端改动重叠会产生冲突，交前端在「变更文件」面板标红。
            if stashed {
                if let Some(conflict) = auto_unstash_after_pull(&repo_path) {
                    return Err(format!(
                        "PULL_CONFLICT: 拉取成功，但还原本地改动时与远端改动冲突，已在「变更文件」面板标红，请直接打开对应文件解决。\n\n{}",
                        conflict
                    ));
                }
            }
            Ok(())
        } else {
            let msg = out2;
            // 分叉（diverging）：--ff-only 拒绝快进。还原 stash 后返回结构化前缀，
            // 前端据此弹「选择拉取方式」对话框，而非直接吐 git 原始的 hint 文本。
            if stashed {
                let _ = auto_unstash_after_pull(&repo_path);
            }
            if msg.contains("Not possible to fast-forward") || msg.contains("Diverging") {
                return Err("PULL_DIVERGED: 本地分支与远程分支已分叉，无法快进合并，请选择合并或变基策略".to_string());
            }
            Err(if msg.trim().is_empty() { "拉取失败".to_string() } else { msg })
        }
    })
    .await
    .map_err(|e| format!("拉取失败: {}", e))?
}

/// 分支分叉时按指定策略拉取：merge（`--no-ff` 生成合并提交）或 rebase（变基到远程之上）。
/// 由前端「选择拉取方式」对话框触发。冲突时返回 `PULL_CONFLICT:` 前缀，供前端引导解决。
#[command]
pub(crate) async fn pull_with_strategy(app: AppHandle, repo_path: String, strategy: String) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_write_guard();

        // 拉取前若工作区有未提交改动，先自动 stash（git 在脏工作区直接 pull 会 abort，无法产生真正冲突）。
        let stashed = auto_stash_before_pull(&repo_path);

        // SSH 私钥自动兜底：同 pull_branch
        let auto_ssh = auto_ssh_key(&repo_path);
        let ssh_for_run = auto_ssh.as_ref().map(|(c, _)| c.as_str());

        // 先 fetch 远程最新代码（流式执行，实时推送拉取进度）
        let (ok, out) = run_git_progress(&app, &repo_path, "fetch", &["--all"], ssh_for_run)
            .map_err(|e| format!("无法执行 git fetch 命令: {}", e))?;
        if !ok {
            let mut msg = out;
            if msg.trim().is_empty() {
                msg = "获取远程代码失败".to_string();
            }
            if stashed {
                let _ = auto_unstash_after_pull(&repo_path);
            }
            return Err(msg);
        }

        // 按策略拉取：merge 生成合并提交（不改写历史），rebase 改写本地历史接到远程之后
        let strategy_arg = if strategy.as_str() == "rebase" {
            "--rebase"
        } else {
            "--no-ff"
        };
        // 同 pull_branch：分支没有 upstream 时显式指定 <remote> <branch>
        let mut pull_args: Vec<String> = vec![strategy_arg.to_string()];
        pull_args.extend(resolve_pull_target(&repo_path));
        let pull_args: Vec<&str> = pull_args.iter().map(|s| s.as_str()).collect();
        let (ok2, out2) = run_git_progress(&app, &repo_path, "pull", &pull_args, ssh_for_run)
            .map_err(|e| format!("无法执行 git pull 命令: {}", e))?;

        if ok2 {
            // 拉取成功：持久化自动检测到的 SSH 私钥（同 pull_branch，仅恰好一把时）
            if let Some((_, Some(key))) = &auto_ssh {
                let _ = apply_ssh_key_config(&repo_path, key);
            }
            // 拉取成功：还原 stash。若与远端改动重叠会产生冲突，交前端在「变更文件」面板标红。
            if stashed {
                if let Some(conflict) = auto_unstash_after_pull(&repo_path) {
                    return Err(format!(
                        "PULL_CONFLICT: 拉取成功，但还原本地改动时与远端改动冲突，已在「变更文件」面板标红，请直接打开对应文件解决。\n\n{}",
                        conflict
                    ));
                }
            }
            Ok(())
        } else {
            let msg = out2;
            if stashed {
                let _ = auto_unstash_after_pull(&repo_path);
            }
            // 冲突：git 留下冲突标记并中止，返回结构化前缀引导前端解决
            if msg.contains("CONFLICT") || msg.to_lowercase().contains("conflict") {
                return Err(format!(
                    "PULL_CONFLICT: 拉取产生冲突，请解决冲突文件后重试。\n\n{}",
                    msg
                ));
            }
            Err(if msg.trim().is_empty() {
                "拉取失败".to_string()
            } else {
                msg
            })
        }
    })
    .await
    .map_err(|e| format!("拉取失败: {}", e))?
}

/// 完成合并：冲突已全部解决并暂存后，提交合并结果（使用 git 自动生成的默认合并信息）。
#[command]
pub(crate) async fn finish_merge(repo_path: String) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_write_guard();
        let output = git_command()
            .arg("-C")
            .arg(&repo_path)
            .arg("commit")
            .arg("--no-edit")
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .map_err(|e| format!("无法执行 git commit 命令: {}", e))?;
        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let msg = format!("{}{}", stderr, stdout);
            Err(if msg.trim().is_empty() {
                "完成合并失败".to_string()
            } else {
                msg
            })
        }
    })
    .await
    .map_err(|e| format!("完成合并失败: {}", e))?
}

/// 继续变基：冲突已解决并暂存后，继续 rebase 流程。
/// 设置 `GIT_EDITOR=true` 跳过交互式编辑器，使用默认提交信息。
#[command]
pub(crate) async fn continue_rebase(repo_path: String) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_write_guard();
        let output = git_command()
            .arg("-C")
            .arg(&repo_path)
            .arg("rebase")
            .arg("--continue")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_EDITOR", "true")
            .output()
            .map_err(|e| format!("无法执行 git rebase 命令: {}", e))?;
        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let msg = format!("{}{}", stderr, stdout);
            Err(if msg.trim().is_empty() {
                "继续变基失败".to_string()
            } else {
                msg
            })
        }
    })
    .await
    .map_err(|e| format!("继续变基失败: {}", e))?
}

/// 中止合并：回退到拉取前的状态（`git merge --abort`）。
#[command]
pub(crate) async fn abort_merge(repo_path: String) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_write_guard();
        let output = git_command()
            .arg("-C")
            .arg(&repo_path)
            .arg("merge")
            .arg("--abort")
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .map_err(|e| format!("无法执行 git merge --abort 命令: {}", e))?;
        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let msg = format!("{}{}", stderr, stdout);
            Err(if msg.trim().is_empty() {
                "中止合并失败".to_string()
            } else {
                msg
            })
        }
    })
    .await
    .map_err(|e| format!("中止合并失败: {}", e))?
}

/// 中止变基：回退到拉取前的状态（`git rebase --abort`）。
#[command]
pub(crate) async fn abort_rebase(repo_path: String) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_write_guard();
        let output = git_command()
            .arg("-C")
            .arg(&repo_path)
            .arg("rebase")
            .arg("--abort")
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .map_err(|e| format!("无法执行 git rebase --abort 命令: {}", e))?;
        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let msg = format!("{}{}", stderr, stdout);
            Err(if msg.trim().is_empty() {
                "中止变基失败".to_string()
            } else {
                msg
            })
        }
    })
    .await
    .map_err(|e| format!("中止变基失败: {}", e))?
}

/// 添加远程仓库（`git remote add <name> <url>`）。
/// 用于「拉取无远程」对话框的"创建远程"按钮。
/// - `name`：远程简称，默认 `origin`（业务层兜底）
/// - `url`：远程仓库 URL（https/ssh/git 协议均可），非空校验由业务层完成
#[command]
pub(crate) async fn add_remote(repo_path: String, name: String, url: String) -> Result<()> {
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
