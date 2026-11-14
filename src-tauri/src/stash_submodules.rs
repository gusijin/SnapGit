use super::*;


// ===== 储藏（Stash）=====

#[derive(serde::Serialize)]
pub(crate) struct StashEntry {
    pub(crate) index: i64,
    pub(crate) branch: String,
    pub(crate) message: String,
    pub(crate) date: String,
    pub(crate) stash_ref: String,
}

#[tauri::command]
pub(crate) async fn stash_create(
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
pub(crate) async fn stash_list(repo_path: String) -> Result<Vec<StashEntry>> {
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
pub(crate) fn regex_capture(line: &str) -> Option<(&str, &str)> {
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
pub(crate) fn parse_stash_rest(rest: &str) -> (String, String) {
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
pub(crate) async fn stash_apply(repo_path: String, stash_ref: String, keep_index: bool) -> Result<()> {
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
pub(crate) async fn stash_drop(repo_path: String, stash_ref: String) -> Result<()> {
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

// ===== 子模块（submodule）管理 =====
// 参考主流 Git GUI（SourceTree / GitKraken / SmartGit 等）的能力：
// 列出子模块及其状态、添加、更新（init / --remote）、同步、移除、在子模块内打开。

/// 运行一个已配置好的 git 命令；成功返回 Ok(())，失败把 stderr+stdout 作为错误信息。
pub(crate) fn run_git(cmd: &mut std::process::Command) -> Result<()> {
    let output = cmd
        .output()
        .map_err(|e| format!("无法执行 git 命令: {}", e))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let msg = format!("{}{}", stderr, stdout);
        Err(if msg.trim().is_empty() {
            "git 命令执行失败".to_string()
        } else {
            msg.trim().to_string()
        })
    }
}

#[command]
pub(crate) async fn list_submodules(repo_path: String) -> Result<Vec<SubmoduleInfo>> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_read_guard();
        list_submodules_inner(&repo_path)
    })
    .await
    .map_err(|e| format!("列出子模块失败: {}", e))?
}

pub(crate) fn list_submodules_inner(repo_path: &str) -> Result<Vec<SubmoduleInfo>> {
    use std::collections::HashMap;

    // 1) 读取 .gitmodules：解析 submodule.<name>.(path|url|branch)
    let mut by_name: HashMap<String, (String, String, Option<String>)> = HashMap::new();
    if let Ok(out) = git_command()
        .arg("-C")
        .arg(repo_path)
        .args(["config", "-f", ".gitmodules", "-l"])
        .output()
    {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                if let Some(rest) = line.strip_prefix("submodule.") {
                    if let Some((key, val)) = rest.split_once('=') {
                        let key = key.trim();
                        let val = val.trim().to_string();
                        let (name, field) = if let Some(n) = key.strip_suffix(".path") {
                            (n.to_string(), 0)
                        } else if let Some(n) = key.strip_suffix(".url") {
                            (n.to_string(), 1)
                        } else if let Some(n) = key.strip_suffix(".branch") {
                            (n.to_string(), 2)
                        } else {
                            continue;
                        };
                        let entry = by_name.entry(name).or_insert((String::new(), String::new(), None));
                        match field {
                            0 => entry.0 = val,
                            1 => entry.1 = val,
                            2 => entry.2 = Some(val),
                            _ => {}
                        }
                    }
                }
            }
        }
    }
    if by_name.is_empty() {
        return Ok(Vec::new());
    }

    // 2) git submodule status：前缀状态 + 当前已检出的短 SHA
    let mut status_map: HashMap<String, (char, Option<String>)> = HashMap::new();
    if let Ok(out) = git_command()
        .arg("-C")
        .arg(repo_path)
        .args(["submodule", "status"])
        .output()
    {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                let line = line.trim_end();
                if line.is_empty() {
                    continue;
                }
                let mut chars = line.chars();
                let prefix = chars.next().unwrap_or(' ');
                let rest = chars.as_str().trim();
                let (p, sha) = if let Some(idx) = rest.find(" (") {
                    (
                        rest[..idx].to_string(),
                        Some(rest[idx + 2..].trim_end_matches(')').to_string()),
                    )
                } else {
                    (rest.to_string(), None)
                };
                status_map.insert(p, (prefix, sha));
            }
        }
    }

    let mut result: Vec<SubmoduleInfo> = Vec::new();
    for (_name, (path, url, branch)) in by_name {
        if path.is_empty() {
            continue;
        }
        let (prefix, _) = status_map.get(&path).cloned().unwrap_or(('-', None));
        let initialized = prefix != '-';

        // 父仓库记录的 gitlink 提交（来自索引）
        let recorded = {
            if let Ok(o) = git_command()
                .arg("-C")
                .arg(repo_path)
                .args(["ls-files", "-s", "--"])
                .arg(&path)
                .output()
            {
                if o.status.success() {
                    String::from_utf8_lossy(&o.stdout)
                        .lines()
                        .next()
                        .and_then(|l| l.split_whitespace().nth(1))
                        .map(|s| s.to_string())
                } else {
                    None
                }
            } else {
                None
            }
        };

        // 已初始化时取子模块真实 HEAD（全量 SHA）
        let head_commit = if initialized {
            if let Ok(o) = git_command()
                .arg("-C")
                .arg(repo_path)
                .args(["-C", &path, "rev-parse", "HEAD"])
                .output()
            {
                if o.status.success() {
                    let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    if s.is_empty() {
                        None
                    } else {
                        Some(s)
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };
        let head_commit_short = head_commit.as_ref().map(|s| s.chars().take(7).collect());

        // 指针改变：已初始化且当前 HEAD 与记录值不一致
        let modified = initialized
            && recorded.is_some()
            && head_commit.as_deref() != recorded.as_deref();

        // 工作树有改动
        let dirty = if initialized {
            if let Ok(o) = git_command()
                .arg("-C")
                .arg(repo_path)
                .args(["-C", &path, "status", "--porcelain"])
                .output()
            {
                o.status.success() && !String::from_utf8_lossy(&o.stdout).trim().is_empty()
            } else {
                false
            }
        } else {
            false
        };

        let display_name = path.split('/').last().unwrap_or(&path).to_string();
        result.push(SubmoduleInfo {
            path,
            name: display_name,
            url,
            branch,
            head_commit,
            head_commit_short,
            recorded_commit: recorded,
            initialized,
            modified,
            dirty,
        });
    }
    result.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(result)
}

#[command]
pub(crate) async fn add_submodule(repo_path: String, url: String, path: String) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_write_guard();
        let mut cmd = git_command();
        cmd.arg("-C").arg(&repo_path).args(["submodule", "add"]);
        if path.trim().is_empty() {
            cmd.arg(&url);
        } else {
            cmd.arg(&url).arg("--").arg(&path);
        }
        cmd.env("GIT_TERMINAL_PROMPT", "0");
        run_git(&mut cmd)
    })
    .await
    .map_err(|e| format!("添加子模块失败: {}", e))?
}

#[command]
pub(crate) async fn update_submodules(repo_path: String, recursive: bool, remote: bool) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_write_guard();
        let mut cmd = git_command();
        cmd.arg("-C").arg(&repo_path).args(["submodule", "update", "--init"]);
        if recursive {
            cmd.arg("--recursive");
        }
        if remote {
            cmd.arg("--remote");
        }
        cmd.env("GIT_TERMINAL_PROMPT", "0");
        run_git(&mut cmd)
    })
    .await
    .map_err(|e| format!("更新子模块失败: {}", e))?
}

#[command]
pub(crate) async fn update_submodule(repo_path: String, path: String, remote: bool) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_write_guard();
        let mut cmd = git_command();
        cmd.arg("-C").arg(&repo_path).args(["submodule", "update", "--init"]);
        if remote {
            cmd.arg("--remote");
        }
        cmd.arg("--").arg(&path);
        cmd.env("GIT_TERMINAL_PROMPT", "0");
        run_git(&mut cmd)
    })
    .await
    .map_err(|e| format!("更新子模块失败: {}", e))?
}

#[command]
pub(crate) async fn sync_submodules(repo_path: String, recursive: bool, path: Option<String>) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_write_guard();
        let mut cmd = git_command();
        cmd.arg("-C").arg(&repo_path).args(["submodule", "sync"]);
        if recursive {
            cmd.arg("--recursive");
        }
        if let Some(p) = path {
            if !p.trim().is_empty() {
                cmd.arg("--").arg(&p);
            }
        }
        run_git(&mut cmd)
    })
    .await
    .map_err(|e| format!("同步子模块失败: {}", e))?
}

#[command]
pub(crate) async fn remove_submodule(repo_path: String, path: String) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_write_guard();
        // 1) 反初始化（已初始化才需要；失败不阻断，后续 git rm 仍可完成移除）
        if let Ok(o) = git_command()
            .arg("-C")
            .arg(&repo_path)
            .args(["submodule", "deinit", "-f", "--"])
            .arg(&path)
            .output()
        {
            let _ = o;
        }
        // 2) 从索引与工作区移除（同时更新 .gitmodules）
        let mut rm = git_command();
        rm.arg("-C").arg(&repo_path).args(["rm", "-f", "--"]).arg(&path);
        run_git(&mut rm)?;
        // 3) 清理 .git/modules 下的数据
        let modules_dir = std::path::Path::new(&repo_path)
            .join(".git")
            .join("modules")
            .join(&path);
        if modules_dir.exists() {
            let _ = std::fs::remove_dir_all(&modules_dir);
        }
        Ok(())
    })
    .await
    .map_err(|e| format!("移除子模块失败: {}", e))?
}
