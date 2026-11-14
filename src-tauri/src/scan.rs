use super::*;


/// 克隆远程仓库到本地目录。
/// - `url`：远程仓库地址（https/ssh/git 协议均可）
/// - `target_dir`：最终目标目录（不存在则 git 自动创建；已存在且必须为空）
/// - `branch`：可选，指定检出分支；空字符串表示使用远程默认分支
/// - `include_submodules`：是否同时初始化并拉取子模块（--recurse-submodules）
/// - `fetch_tags`：是否获取所有标签（--tags）
/// 返回克隆后的本地绝对路径（与传入 target_dir 一致）
#[command]
pub(crate) async fn clone_repository(
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
pub(crate) async fn create_branch(repo_path: String, branch_name: String) -> Result<()> {
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
pub(crate) async fn get_current_branch(repo_path: String) -> Result<String> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_read_guard();
        let repo = Repository::open(&repo_path).map_err(|e| e.to_string())?;

        let head = repo.head().map_err(|e| e.to_string())?;
        // 游离 HEAD：HEAD 直接指向 commit，无分支名（symbolic_target 为 None），
        // 返回空串供前端显示「游离 HEAD」
        if head.symbolic_target().is_none() {
            return Ok(String::new());
        }
        Ok(head.shorthand().unwrap_or("").to_string())
    })
    .await
    .map_err(|e| format!("获取当前分支失败: {}", e))?
}

#[command]
pub(crate) async fn merge_branch(repo_path: String, branch_name: String) -> Result<()> {
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
pub(crate) async fn rename_branch(repo_path: String, old_name: String, new_name: String) -> Result<()> {
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
pub(crate) async fn delete_branch(
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
pub(crate) async fn delete_remote_branch(
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
pub(crate) fn detect_default_branch(repo_path: &str, remote: &str) -> Option<String> {
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
pub(crate) async fn check_remote_branch_deletable(
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
pub(crate) fn open_folder_dialog() -> Result<Option<String>> {
    let _guard = git_read_guard();
    let result = rfd::FileDialog::new().pick_folder();

    match result {
        Some(path) => Ok(Some(path.to_string_lossy().to_string())),
        None => Ok(None),
    }
}

#[command]
pub(crate) fn open_file_dialog() -> Result<Option<String>> {
    let _guard = git_read_guard();
    match rfd::FileDialog::new().pick_file() {
        Some(path) => Ok(Some(path.to_string_lossy().to_string())),
        None => Ok(None),
    }
}

// 在仓库目录打开系统终端（命令行 / git shell）
#[command]
pub(crate) fn open_in_terminal(repo_path: String) -> Result<()> {
    let raw = repo_path.trim().to_string();
    // Windows 扩展长度路径前缀（\\?\）会让 start 解析异常，剥掉
    let path = raw.strip_prefix(r"\\?\").unwrap_or(&raw).to_string();
    if !Path::new(&path).is_dir() {
        return Err(format!("仓库路径不存在或不是目录: {}", path));
    }

    #[cfg(target_os = "windows")]
    {
        // 优先用 Git Bash（自带 git + Linux 命令，比裸 cmd 顺手）
        // git-bash.exe 的 --cd=DIR 可在新窗口中直接定位到仓库目录
        let git_bash_candidates = [
            "C:\\Program Files\\Git\\git-bash.exe",
            "C:\\Program Files (x86)\\Git\\git-bash.exe",
        ];
        if let Some(git_bash) = git_bash_candidates.iter().find(|p| Path::new(p).exists()) {
            // MSYS 对路径分隔符更友好，反斜杠统一转正斜杠再喂给 --cd
            let unix_path = path.replace('\\', "/");
            let child = std::process::Command::new(git_bash)
                .arg(format!("--cd={}", unix_path))
                .creation_flags(0x00000010)
                .spawn();
            return child.map(|_| ()).map_err(|e| format!("无法打开 Git Bash: {}", e));
        }
        // 回退：新开一个 cmd 窗口并 cd 到仓库目录（Git for Windows 的 git.exe 已在 PATH 上）
        // /d 指定起始目录；CREATE_NEW_CONSOLE(0x10) 让其独立成窗口
        let child = std::process::Command::new("cmd")
            .args(["/c", "start", "/d", &path, "cmd"])
            .creation_flags(0x00000010)
            .spawn();
        return child.map(|_| ()).map_err(|e| format!("无法打开终端: {}", e));
    }

    #[cfg(target_os = "macos")]
    {
        // 用系统 Terminal 打开仓库目录（新窗口自动定位到该路径）
        let child = std::process::Command::new("open")
            .args(["-a", "Terminal", &path])
            .spawn();
        return child.map(|_| ()).map_err(|e| format!("无法打开终端: {}", e));
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        Err("当前平台不支持打开终端".to_string())
    }
}

#[command]
pub(crate) async fn set_ssh_key_path(repo_path: String, key_path: String) -> Result<String> {
    let _guard = git_read_guard();
    apply_ssh_key_config(&repo_path, &key_path)
}

// 保存仓库用户信息（user.name / user.email）。
// 空字符串表示移除该配置（git config --unset），方便回退到 global/system 默认值。
#[command]
pub(crate) async fn set_user_identity(repo_path: String, name: String, email: String) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_read_guard();
        let set_or_unset = |key: &str, value: &str| -> std::result::Result<(), String> {
            let mut cmd = git_command();
            cmd.arg("-C").arg(&repo_path).arg("config").arg("--local");
            if value.trim().is_empty() {
                cmd.arg("--unset").arg(key);
            } else {
                cmd.arg(key).arg(value.trim());
            }
            match cmd.status() {
                Ok(s) if s.success() => Ok(()),
                Ok(s) => Err(format!("git config {} 写入失败，退出码: {}", key, s)),
                Err(e) => Err(format!("执行 git 失败: {}", e)),
            }
        };
        set_or_unset("user.name", &name)?;
        set_or_unset("user.email", &email)?;
        Ok(())
    })
    .await
    .map_err(|e| format!("保存用户信息失败: {}", e))?
}

// 检测系统是否可用 git：优先 PATH，Windows 额外探测常见安装路径。
// 返回 { available, version, path }，供前端判断是否需要引导用户安装 Git。
#[command]
pub(crate) fn detect_git() -> Result<serde_json::Value> {
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

pub(crate) fn get_scan_directories() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    let home_path = PathBuf::from(&home);

    // 通用开发目录（Windows / macOS / Linux 共有或近似通用）。
    // 注：剔除 Desktop / Downloads / Library 等体量巨大且无仓库意义的目录，
    // 深度递归扫描会严重拖慢首屏。
    let common = [
        "workspace", "code", "repos", "projects",
        "opensource", "sandbox", "dev", "github",
        "gitee", "source",
    ];

    // 平台专属目录：Windows 用反斜杠子路径；macOS / Linux 用其惯例开发目录。
    // macOS 上很多开发者把仓库放在 ~/Developer（Xcode 默认）、~/git、~/src、
    // ~/Documents、~/Projects、~/Code 等；原实现只列了 Windows 风格目录，
    // 导致 macOS 上候选目录全不存在、扫描结果为空（"不会自动扫描到仓库"）。
    #[cfg(target_os = "windows")]
    let platform = [
        "Documents\\site", "Documents\\projects",
        "Documents\\code", "Documents\\workspace",
    ];
    #[cfg(not(target_os = "windows"))]
    let platform = [
        "Developer", "git", "src", "Projects", "Code",
        "Repos", "Workspace", "Documents",
    ];

    for sub in common.iter().chain(platform.iter()) {
        let p = home_path.join(sub);
        if p.exists() {
            dirs.push(p);
        }
    }

    // 当前项目目录的上级（打开过仓库后，同级目录也很可能还有仓库）
    if let Ok(current) = std::env::current_dir() {
        if let Some(parent) = current.parent() {
            dirs.push(parent.to_path_buf());
        }
    }

    dirs
}

// Windows 上 std::fs::canonicalize 会返回带 \\?\ 前缀的扩展长度路径，
// 展示给用户前需还原成常规路径（\\?\C:\... → C:\...；\\?\UNC\... → \\...）。
pub(crate) fn normalize_windows_path(p: &Path) -> PathBuf {
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

pub(crate) fn is_skip_dir(name: &str) -> bool {
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

pub(crate) fn scan_directory(
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
pub(crate) fn scan_projects() -> Result<Vec<ScannedProject>> {
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

pub(crate) fn build_file_tree(
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
pub(crate) async fn get_file_tree(repo_path: String) -> Result<Vec<FileTreeNode>> {
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
