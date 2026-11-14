#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
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
use tauri::{AppHandle, Emitter, Listener, Manager, Runtime, TitleBarStyle, WebviewUrl, WebviewWindow, WebviewWindowBuilder, Window};

type Result<T> = std::result::Result<T, String>;
mod git_core;

mod models;

mod repository;

mod history;

mod mutations;

mod checkout;

mod scan;

mod diff;

mod stash_submodules;

mod edit_menu;

pub(crate) use git_core::*;
pub(crate) use models::*;
pub(crate) use repository::*;
pub(crate) use history::*;
pub(crate) use mutations::*;
pub(crate) use checkout::*;
pub(crate) use scan::*;
pub(crate) use diff::*;
pub(crate) use stash_submodules::*;
pub(crate) use edit_menu::*;


fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            // 主窗口：macOS 保留系统标题栏（红黄绿交通灯），其余平台无边框 +
            // 前端自定义标题栏（TitleBar.vue）。conf.json 不再定义此窗口，避免
            // decorations:false 误把 macOS 交通灯也关掉（与编辑窗口保持一致的平台区分）。
            // macOS 用 Transparent 标题栏样式：原生透明标题栏，本身可拖拽（无需前端拖拽区），
            // 用窗口背景色 (= 应用头部色 #e2e8f0) 把原生标题栏染成与应用头部一致，视觉无缝。
            // 不使用 Overlay：Overlay 的 fullsize 内容视图会把顶部事件截走、且 hidden_title 后
            // 不响应拖拽，导致窗口无法拖动。
            #[cfg(target_os = "macos")]
            let _main = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("SnapGit")
                .inner_size(1200.0, 800.0)
                .resizable(true)
                .title_bar_style(TitleBarStyle::Transparent)
                .hidden_title(true)
                .background_color(tauri::window::Color::from((226, 232, 240, 255)))
                .build()?;

            #[cfg(not(target_os = "macos"))]
            let _main = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("SnapGit")
                .inner_size(1200.0, 800.0)
                .resizable(true)
                .decorations(false)
                // WebView2 冷启动要几秒才出首帧，期间整窗显示窗口背景色。
                // 设为应用头部色 #e2e8f0（与 macOS 一致），避免「启动后全白几秒」的割裂感。
                .background_color(tauri::window::Color::from((226, 232, 240, 255)))
                .build()?;

            // macOS: 使用系统原生菜单栏（显示在屏幕顶部）
            // Windows/Linux: 不设置原生菜单，由前端 Vue 组件 MenuBar.vue 渲染窗口内菜单栏
            #[cfg(target_os = "macos")]
            {
                // 构建原生菜单栏（含语言切换项，用于同步勾选态）
                let (menu, lang_items, sidebar_item, submodules_item) = build_menu(&app.handle())?;
                app.set_menu(menu)?;

                // 语言项引用：菜单事件与前端 locale-changed 事件共用，各自持有一份克隆
                let lang_for_menu = lang_items.clone();
                let lang_for_state = lang_items;
                // 管理状态，供 locale-changed 监听回写勾选态
                app.manage(LangMenuState { items: lang_for_state });

                let app_handle = app.handle().clone();
                app.on_menu_event(move |app, event| {
                    let id = event.id().0.clone();
                    if id.starts_with("set-locale-") {
                        // 更新原生菜单勾选态，使当前语言保持勾选
                        for it in &lang_for_menu {
                            let _ = it.set_checked(it.id().0 == id);
                        }
                        if let Some(win) = app.get_webview_window("main") {
                            let _ = win.emit("menu-action", serde_json::json!({ "action": id }));
                        }
                        return;
                    }
                    if let Some(win) = app.get_webview_window("main") {
                        on_menu_event(&app_handle, &win, &id);
                    }
                });

                // 前端切换语言（含启动时初始同步）后回传，保持原生菜单勾选态一致
                let app_handle2 = app.handle().clone();
                app.listen("locale-changed", move |event| {
                    let payload: serde_json::Value =
                        serde_json::from_str(event.payload()).unwrap_or(serde_json::Value::Null);
                    if let Some(code) = payload.get("locale").and_then(|v| v.as_str()) {
                        let target = format!("set-locale-{}", code);
                        let state = app_handle2.state::<LangMenuState>();
                        for it in &state.items {
                            let _ = it.set_checked(it.id().0 == target);
                        }
                    }
                });

                // 前端侧边栏折叠/展开后回传，保持原生菜单「侧边栏」勾选态一致
                app.listen("sidebar-toggled", move |event| {
                    let payload: serde_json::Value =
                        serde_json::from_str(event.payload()).unwrap_or(serde_json::Value::Null);
                    if let Some(collapsed) = payload.get("collapsed").and_then(|v| v.as_bool()) {
                        let _ = sidebar_item.set_checked(!collapsed);
                    }
                });

                // 前端「视图 → 子模块」切换后回传，保持原生菜单勾选态一致；
                // 前端启动时也会同步一次，用于恢复 localStorage 里保存的显隐偏好。
                app.listen("submodules-toggled", move |event| {
                    let payload: serde_json::Value =
                        serde_json::from_str(event.payload()).unwrap_or(serde_json::Value::Null);
                    if let Some(visible) = payload.get("visible").and_then(|v| v.as_bool()) {
                        let _ = submodules_item.set_checked(visible);
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
            stage_files,
            discard_file,
            commit,
            push,
            get_remote_url,
            save_credentials,
            save_github_token,
            get_repo_config,
            checkout_branch,
            checkout_remote_branch,
            pull_branch,
            pull_with_strategy,
            finish_merge,
            continue_rebase,
            abort_merge,
            abort_rebase,
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
            open_in_terminal,
            set_ssh_key_path,
            set_user_identity,
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
            list_submodules,
            add_submodule,
            update_submodules,
            update_submodule,
            sync_submodules,
            remove_submodule,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod diff_tests;
