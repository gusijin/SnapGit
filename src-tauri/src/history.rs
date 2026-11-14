use super::*;


#[command]
pub(crate) async fn get_commits(repo_path: String, limit: usize, skip: Option<usize>) -> Result<Vec<Commit>> {
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

        // 预先建好「提交 id → 引用列表」映射：本地分支优先，其次远程分支，最后标签。
        // 每次分页都重新扫一遍引用（引用数量远少于提交，开销可忽略），
        // 这样即使期间切分支/拉取，下一页的标记也是最新的。
        let mut refs_by_oid: HashMap<git2::Oid, Vec<CommitRef>> = HashMap::new();
        if let Ok(references) = repo.references() {
            for reference in references.flatten() {
                if reference.symbolic_target().is_some() {
                    continue;
                }
                let Some(full_name) = reference.name() else { continue };
                let (name, ref_type) = if let Some(n) = full_name.strip_prefix("refs/heads/") {
                    (n, "branch")
                } else if let Some(n) = full_name.strip_prefix("refs/remotes/") {
                    // origin/HEAD 只是默认分支的符号引用，不展示
                    if n.ends_with("/HEAD") {
                        continue;
                    }
                    (n, "remote")
                } else if let Some(n) = full_name.strip_prefix("refs/tags/") {
                    (n, "tag")
                } else {
                    continue;
                };
                if let Ok(commit) = reference.peel_to_commit() {
                    refs_by_oid.entry(commit.id()).or_default().push(CommitRef {
                        name: name.to_string(),
                        ref_type: ref_type.to_string(),
                    });
                }
            }
        }
        // 稳定排序：分支 → 远程 → 标签，同类按名称
        for refs in refs_by_oid.values_mut() {
            refs.sort_by(|a, b| {
                let rank = |t: &str| match t {
                    "branch" => 0,
                    "remote" => 1,
                    _ => 2,
                };
                rank(&a.ref_type)
                    .cmp(&rank(&b.ref_type))
                    .then_with(|| a.name.cmp(&b.name))
            });
        }

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
                parents: commit.parent_ids().map(|p| p.to_string()).collect(),
                refs: refs_by_oid.remove(&id).unwrap_or_default(),
            });
        }

        Ok(commits)
    })
    .await
    .map_err(|e| format!("加载提交记录失败: {}", e))?
}

// 本地分支未配置上游时，对比第一个远程的 HEAD（默认分支）估算"本地需要上传"的提交数
// 只取 ahead 分量（behind 置 0），避免未配置上游的分支显示误导性的"落后"数
pub(crate) fn remote_head_ahead(repo: &Repository, local_oid: git2::Oid) -> (usize, usize) {
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
pub(crate) async fn get_branches(repo_path: String, full: Option<bool>) -> Result<Vec<Branch>> {
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
pub(crate) async fn get_remotes(repo_path: String) -> Result<Vec<String>> {
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
pub(crate) async fn get_upstream(repo_path: String, branch_name: String) -> Result<Option<UpstreamInfo>> {
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
pub(crate) fn is_conflict_status(x: char, y: char) -> bool {
    matches!(
        (x, y),
        ('U', 'U') | ('A', 'A') | ('D', 'D') | ('A', 'U') | ('U', 'A') | ('D', 'U') | ('U', 'D')
    )
}

// 将 libgit2 的 Status 位掩码映射到与 `git status --porcelain` 等价的 (x, y) 字符，
// 以便复用原有状态字符串映射，保证 UI 展示与旧 CLI 实现完全一致。
pub(crate) fn status_xy(status: Status) -> (char, char) {
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
pub(crate) fn map_status(x: char, y: char) -> Option<&'static str> {
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
pub(crate) async fn get_file_status(repo_path: String, cached: Option<bool>) -> Result<Vec<FileStatus>> {
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
