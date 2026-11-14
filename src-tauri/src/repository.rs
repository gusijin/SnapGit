use super::*;


pub(crate) fn get_recent_file_path(app: &AppHandle) -> PathBuf {
    let app_data_dir = app.path().app_data_dir().expect("Failed to get app data directory");
    fs::create_dir_all(&app_data_dir).expect("Failed to create app data directory");
    app_data_dir.join("recent_repos.json")
}

#[command]
pub(crate) fn load_recent_repositories(app: AppHandle) -> Result<Vec<RepositoryInfo>> {
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
pub(crate) fn save_recent_repository(app: AppHandle, repo_info: RepositoryInfo) -> Result<()> {
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
pub(crate) fn remove_recent_repository(app: AppHandle, path: String) -> Result<()> {
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
pub(crate) fn open_repository(path: String) -> Result<RepositoryInfo> {
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

    // HEAD 可能因"空仓库（刚 git init 尚未提交）"而处于 unborn 状态，head() 会报错。
    // 此时不致命，默认回退到 main，避免打开空仓库时被当作通用错误静默吞掉。
    let current_branch = repo
        .head()
        .ok()
        .and_then(|h| h.shorthand().map(|s| s.to_string()))
        .unwrap_or_else(|| "main".to_string());

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
pub(crate) fn init_repository(repo_path: String) -> Result<RepositoryInfo> {
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
