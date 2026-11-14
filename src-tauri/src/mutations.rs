use super::*;


// 暂存单个路径，返回是否真的有内容可暂存。
// 用 git add -A 一次性处理新增、修改、删除等所有变更。
// 当路径在工作区和索引里都不存在时（删除已被暂存、文件被外部改名/删除、
// 前端列表来自旧快照），git 会报 "pathspec ... did not match any files"。
// 这只代表「该路径无事可做」，不是失败：若当成错误抛出，一个幽灵路径会让整批提交中止。
pub(crate) fn stage_one_path(repo_path: &str, file_path: &str) -> Result<bool> {
    let output = git_command()
        .arg("-C")
        .arg(repo_path)
        .arg("add")
        .arg("-A")
        .arg("--")
        .arg(file_path)
        .output()
        .map_err(|e| format!("无法执行 git add 命令: {}", e))?;

    if output.status.success() {
        return Ok(true);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let msg = format!("{}{}", stderr, stdout);
    if msg.contains("did not match any files") {
        return Ok(false);
    }
    Err(if msg.trim().is_empty() { "暂存失败".to_string() } else { msg })
}

#[command]
pub(crate) async fn stage_file(repo_path: String, file_path: String) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_write_guard();
        stage_one_path(&repo_path, &file_path).map(|_| ())
    })
    .await
    .map_err(|e| format!("暂存文件失败: {}", e))?
}

/// 批量暂存：一次拿写锁、一次 IPC，避免前端逐个 invoke。
/// 返回被跳过的路径（已无内容可暂存），供前端判断「全部消失」而不是报错。
#[command]
pub(crate) async fn stage_files(repo_path: String, file_paths: Vec<String>) -> Result<Vec<String>> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_write_guard();
        let mut skipped = Vec::new();
        for file_path in &file_paths {
            if !stage_one_path(&repo_path, file_path)? {
                skipped.push(file_path.clone());
            }
        }
        Ok(skipped)
    })
    .await
    .map_err(|e| format!("暂存文件失败: {}", e))?
}

#[command]
pub(crate) async fn discard_file(repo_path: String, file_path: String) -> Result<()> {
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
pub(crate) async fn commit(repo_path: String, message: String) -> Result<String> {
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
pub(crate) async fn push(repo_path: String, remote: String, local_branch: String, remote_branch: String) -> Result<()> {
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
            // 区分认证失败类型，给出更精准的提示。
            // 注意：SSH 公钥失败不走「需填令牌」分支，否则已配 SSH 私钥却仍被逼填令牌（与「二选一即可」矛盾）。
            let lower = msg.to_lowercase();
            if lower.contains("permission denied (publickey)") || lower.contains("could not read from remote repository") {
                msg = format!(
                    "推送失败：SSH 公钥认证未通过\n\n已配置的 SSH 私钥未能通过 GitHub 认证，请检查：\n  · 该私钥对应的公钥是否已添加到 GitHub 账户的 SSH keys\n  · 私钥文件路径与权限是否正确（私钥权限应为 600）\n  · 本机 ~/.ssh/known_hosts 是否已包含 github.com\n\n原始错误:\n{}",
                    msg,
                );
            } else if msg.contains("Authentication failed") || msg.contains("could not read Username") || msg.contains("terminal prompts disabled") {
                msg = format!("推送失败：远程仓库认证失败\n\n请确保已配置认证方式（SSH 密钥或 credential helper / 访问令牌）。\n\n原始错误:\n{}", msg);
            }
            Err(msg)
        }
    })
    .await
    .map_err(|e| format!("推送失败: {}", e))?
}

#[command]
pub(crate) async fn get_remote_url(repo_path: String, remote_name: String) -> Result<String> {
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
pub(crate) async fn get_repo_config(repo_path: String) -> Result<serde_json::Value> {
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
                // 避免 "home.join(\".ssh/id_ed25519\")" 保留正斜杠导致 Windows 路径混用斜杠
                p.split('/').fold(home.clone(), |acc, s| acc.join(s)).to_string_lossy().to_string()
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
pub(crate) async fn save_credentials(repo_path: String, remote_name: String, username: String, token: String) -> Result<()> {
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

/// 全局「登录 GitHub」：把 OAuth 设备流拿到的 access_token 存入 git 凭证库（针对 https://github.com）。
/// 这样任意 github.com 的 HTTPS 远端在 push/fetch 时都能自动复用，无需每次推送都重新授权。
/// 仅在尚未配置任何 credential.helper 时启用 store（明文存于 ~/.git-credentials，
/// 与 save_credentials 的本地配置一致），避免覆盖用户已有的凭证助手（如 osxkeychain / GCM）。
#[command]
pub(crate) async fn save_github_token(token: String) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        // 若未配置任何 credential.helper，则启用 store（与 save_credentials 的本地配置同源）
        let has_helper = git_command()
            .arg("config")
            .arg("--global")
            .arg("--get")
            .arg("credential.helper")
            .output()
            .map(|o| o.status.success() && !String::from_utf8_lossy(&o.stdout).trim().is_empty())
            .unwrap_or(false);
        if !has_helper {
            let _ = git_command()
                .arg("--global")
                .arg("config")
                .arg("credential.helper")
                .arg("store")
                .output();
        }

        let mut child = git_command()
            .arg("credential")
            .arg("approve")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("无法执行 git credential: {}", e))?;

        if let Some(stdin) = child.stdin.as_mut() {
            use std::io::Write;
            write!(stdin, "url=https://github.com\n").ok();
            write!(stdin, "username=x-access-token\n").ok();
            write!(stdin, "password={}\n", token).ok();
        }

        let output = child.wait_with_output().map_err(|e| format!("等待失败: {}", e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("保存 GitHub 凭证失败: {}", stderr));
        }

        Ok(())
    })
    .await
    .map_err(|e| format!("保存 GitHub 凭证失败: {}", e))?
}
