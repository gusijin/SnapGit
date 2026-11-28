use super::*;

// 在独立的新窗口中打开文件编辑/冲突解决器（不再用当前窗口的 Dialog 弹窗）。
// 传参方式：
// 1. URL hash 标记为 #/editor，让前端 main.ts 知道该挂载 EditorWindow。
// 2. 参数存入 EDIT_ARGS_MAP，前端 mount 后调用 get_edit_args(label) 主动获取。
// 3. 仍尝试 initialization_script 注入 window.__SNAPGIT_EDIT_ARGS__ 作为加速路径。
// 注意：WebviewUrl::App 不要带 search query，否则 dev 模式会触发
// "could not find webview label tauri.localhost" 并加载空白页。
#[command]
pub(crate) async fn open_edit_window(
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
    let current_index_value =
        current_index.filter(|&i| files_list.as_ref().is_some_and(|list| i < list.len()));
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
        let builder =
            tauri::WebviewWindowBuilder::new(&app, &label, WebviewUrl::App("editor.html".into()))
                .title("SnapGit - 文件编辑")
                .inner_size(1200.0, 800.0)
                .min_inner_size(700.0, 500.0)
                .resizable(true)
                // Windows/Linux: 关闭系统标题栏，由前端 TitleBar.vue 自绘（跟随主题切换）。
                // macOS 分支见下，改用 TitleBarStyle::Overlay 让内容撑满、交通灯浮层覆盖（无空白行）。
                .decorations(false)
                // 窗口背景色 = 应用头部色 #e2e8f0：WebView2 首帧前的空窗期不再闪白。
                .background_color(tauri::window::Color::from((226, 232, 240, 255)))
                .initialization_script(&script);

        #[cfg(target_os = "macos")]
        let builder =
            tauri::WebviewWindowBuilder::new(&app, &label, WebviewUrl::App("editor.html".into()))
                .title("SnapGit - 文件编辑")
                .inner_size(1200.0, 800.0)
                .min_inner_size(700.0, 500.0)
                .resizable(true)
                // macOS: Transparent 标题栏样式（原生透明标题栏，本身可拖拽），
                // 用窗口背景色 (= 应用头部色 #e2e8f0) 染成与应用头部一致，视觉无缝；
                // 不使用 Overlay 以免 fullsize 内容视图截走顶部事件、导致无法拖拽窗口。
                .title_bar_style(TitleBarStyle::Transparent)
                .hidden_title(true)
                .background_color(tauri::window::Color::from((226, 232, 240, 255)))
                .initialization_script(&script);

        // Windows：编辑窗口与主窗口保持一致，绕开 GPU 渲染挂起导致的白屏。
        // 仅在 Windows 生效；macOS 分支下这条语句不参与编译，Linux 同理忽略。
        #[cfg(target_os = "windows")]
        let builder = builder.additional_browser_args(
            "--disable-gpu --disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection",
        );

        builder.build().map_err(|e| e.to_string())?
    };

    Ok(())
}

// 供独立编辑窗口前端获取打开时传入的参数。
// 支持三种查找方式（依次尝试）：
// 1. 前端传入显式 label（最可靠）
// 2. 用调用方 webview 的 label 自动匹配
// 3. 回退搜索：遍历 EDIT_ARGS_MAP 找第一个 editor-* 条目（兜底）
#[command]
pub(crate) fn get_edit_args(
    window: WebviewWindow,
    label: Option<String>,
) -> Result<serde_json::Value> {
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
pub(crate) fn close_edit_window(app: AppHandle, window: Window) -> Result<()> {
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.emit("file-edited", ());
    }
    window.close().map_err(|e| e.to_string())?;
    Ok(())
}

#[command]
pub(crate) fn get_commit_files(repo_path: String, commit_id: String) -> Result<Vec<FileStatus>> {
    let _guard = git_read_guard();
    let repo = Repository::open(&repo_path).map_err(|e| e.to_string())?;

    let oid = git2::Oid::from_str(&commit_id).map_err(|e| e.to_string())?;
    let commit = repo.find_commit(oid).map_err(|e| e.to_string())?;
    let tree = commit.tree().map_err(|e| e.to_string())?;

    let parents: Vec<git2::Tree> = commit.parents().filter_map(|p| p.tree().ok()).collect();

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
        })
        .map_err(|e| e.to_string())?;
    } else {
        // 与第一个父提交比较
        let parent_tree = &parents[0];
        let mut diff_opts = DiffOptions::new();
        diff_opts.include_untracked(true);

        let diff = repo
            .diff_tree_to_tree(Some(parent_tree), Some(&tree), Some(&mut diff_opts))
            .map_err(|e| e.to_string())?;

        for delta in diff.deltas() {
            let path = delta
                .new_file()
                .path()
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

// 原生菜单栏已拆分到 native_menu.rs（仅 macOS 编译），本文件只保留编辑窗口与相关命令。
