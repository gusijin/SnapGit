use super::*;

/// Recent repositories are application state rather than Git state. Keep a
/// dedicated mutex so two UI events cannot truncate the JSON file together.
pub(crate) static RECENT_REPOS_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

pub(crate) fn get_recent_file_path(app: &AppHandle) -> Result<PathBuf> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法获取应用数据目录: {}", e))?;
    fs::create_dir_all(&app_data_dir).map_err(|e| format!("无法创建应用数据目录: {}", e))?;
    Ok(app_data_dir.join("recent_repos.json"))
}

fn read_recent_repositories(file_path: &Path) -> Result<Vec<RepositoryInfo>> {
    if !file_path.exists() {
        return Ok(Vec::new());
    }

    let file = File::open(file_path).map_err(|e| e.to_string())?;
    let reader = BufReader::new(file);
    let recent: RecentRepositories = serde_json::from_reader(reader).map_err(|e| e.to_string())?;
    Ok(recent.repos)
}

fn write_recent_repositories(file_path: &Path, repos: Vec<RepositoryInfo>) -> Result<()> {
    let temp_path = file_path.with_extension("json.tmp");
    let file = File::create(&temp_path).map_err(|e| e.to_string())?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer(&mut writer, &RecentRepositories { repos }).map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())?;
    writer.get_ref().sync_all().map_err(|e| e.to_string())?;

    // Windows cannot rename over an existing file. Removing the old file is a
    // small fallback window, while the temporary file still prevents partial
    // JSON from being exposed after a failed write.
    if cfg!(windows) && file_path.exists() {
        fs::remove_file(file_path).map_err(|e| e.to_string())?;
    }
    fs::rename(&temp_path, file_path).map_err(|e| e.to_string())
}

#[command]
pub(crate) fn load_recent_repositories(app: AppHandle) -> Result<Vec<RepositoryInfo>> {
    let _file_guard = RECENT_REPOS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let file_path = get_recent_file_path(&app)?;
    read_recent_repositories(&file_path)
}

#[command]
pub(crate) fn save_recent_repository(app: AppHandle, repo_info: RepositoryInfo) -> Result<()> {
    let _file_guard = RECENT_REPOS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let file_path = get_recent_file_path(&app)?;
    let mut repos = read_recent_repositories(&file_path)?;

    repos.retain(|r| r.path != repo_info.path);
    repos.insert(0, repo_info);
    repos.truncate(10);
    write_recent_repositories(&file_path, repos)
}

#[command]
pub(crate) fn remove_recent_repository(app: AppHandle, path: String) -> Result<()> {
    let _file_guard = RECENT_REPOS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let file_path = get_recent_file_path(&app)?;
    let mut repos = read_recent_repositories(&file_path)?;
    repos.retain(|r| r.path != path);
    write_recent_repositories(&file_path, repos)
}

#[command]
pub(crate) fn open_repository(path: String) -> Result<RepositoryInfo> {
    let _guard = git_read_guard();
    let repo = open_repo_raw(&path).map_err(|e| {
        let msg = e.to_string();
        if matches!(e.code(), git2::ErrorCode::NotFound) {
            format!("NOT_A_REPO:{}", msg)
        } else {
            msg
        }
    })?;
    // 用闭包包住后续逻辑：所有 `?` 都先回到这里，
    // 保证取出的仓库实例一定归还 REPO_CACHE（open_repo 是 remove 语义）。
    let result = (|| -> Result<RepositoryInfo> {
        let repo_name = PathBuf::from(&path)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let current_branch = repo
            .head()
            .ok()
            .and_then(|h| h.shorthand().map(|s| s.to_string()))
            .unwrap_or_else(|| "main".to_string());

        Ok(RepositoryInfo {
            path: path.clone(),
            name: repo_name,
            current_branch,
        })
    })();
    return_repo(&path, repo);
    result
}

/// Initialize a directory as a new Git repository using `main` as its initial branch.
#[command]
pub(crate) fn init_repository(repo_path: String) -> Result<RepositoryInfo> {
    let _guard = git_write_guard();

    let path_buf = PathBuf::from(&repo_path);
    if !path_buf.is_dir() {
        return Err(format!("路径 {} 不是有效目录", repo_path));
    }

    let mut opts = git2::RepositoryInitOptions::new();
    opts.initial_head("main");
    opts.mkpath(true);
    opts.mkdir(true);
    Repository::init_opts(&repo_path, &opts).map_err(|e| format!("初始化仓库失败: {}", e))?;

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
