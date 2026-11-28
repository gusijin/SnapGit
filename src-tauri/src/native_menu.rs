// ===== macOS 系统原生菜单栏（Windows/Linux 使用前端 MenuBar.vue）=====
//
// 设计要点：
// 1. 菜单只在启动时构建一次，之后所有状态通过前端统一的 `menu-state` 事件更新，
//    不再为每个状态各开一个事件，也不重建菜单（避免 macOS 主线程上的菜单替换）。
// 2. 语言项由前端 LocaleRegistry 动态下发，Rust 里不再硬编码语言清单：
//    新增语言只需改 src/i18n/locales/index.ts，菜单会自动增删子项。
// 3. 菜单文案同样由前端按当前界面语言下发，切英文界面时原生菜单一起变。
// 4. 编辑菜单必须保留标准粘贴板项，否则 macOS 的 Cmd+C/V/X/A/Z 不会路由到 WebView。

use std::sync::Mutex;
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Emitter, Listener, Manager, Wry};

type App = AppHandle<Wry>;

/// 编辑菜单中语言子项之前的固定项数量：
/// 撤销 / 重做 / 分隔 / 剪切 / 复制 / 粘贴 / 全选 / 分隔 / 配置 / 分隔 = 10。
/// 语言列表变化时从这里 remove_at 再 append，其余项不受影响。
const LANG_START: usize = 10;

/// 启动时的占位语言列表。前端 onMounted 会立刻 emit `menu-state` 覆盖成
/// LocaleRegistry 的真实列表，因此这里无需与前端语言表保持同步。
fn default_locales() -> Vec<(String, String)> {
    vec![
        ("zh-CN".into(), "简体中文".into()),
        ("en-US".into(), "English".into()),
    ]
}

/// 支持本地化文案的菜单项：四类都实现 set_text(&str)。
enum TextItem {
    Normal(MenuItem<Wry>),
    Check(CheckMenuItem<Wry>),
    Submenu(Submenu<Wry>),
    Predefined(PredefinedMenuItem<Wry>),
}

impl TextItem {
    fn set_text(&self, text: &str) {
        let _ = match self {
            TextItem::Normal(i) => i.set_text(text),
            TextItem::Check(i) => i.set_text(text),
            TextItem::Submenu(i) => i.set_text(text),
            TextItem::Predefined(i) => i.set_text(text),
        };
    }
}

/// 由 app.manage 持有的菜单引用，供 `menu-state` / 菜单点击事件读写勾选态。
pub(crate) struct NativeMenuState {
    lang_items: Mutex<Vec<CheckMenuItem<Wry>>>,
    edit_menu: Submenu<Wry>,
    worktree_item: CheckMenuItem<Wry>,
    log_item: CheckMenuItem<Wry>,
    log_panel_item: CheckMenuItem<Wry>,
    sidebar_item: CheckMenuItem<Wry>,
    submodules_item: CheckMenuItem<Wry>,
    theme_item: CheckMenuItem<Wry>,
    /// (i18n key, 菜单项)：前端切换语言时按 key 逐个 set_text。
    text_items: Vec<(String, TextItem)>,
}

fn build_menu(
    app: &App,
    locales: &[(String, String)],
) -> tauri::Result<(Menu<Wry>, NativeMenuState)> {
    // —— 文件 ——
    let open_repo_item = MenuItem::with_id(app, "open-repo", "打开仓库", true, None::<&str>)?;
    let clone_repo_item = MenuItem::with_id(app, "clone-repo", "克隆仓库", true, None::<&str>)?;
    let quit_item = PredefinedMenuItem::quit(app, Some("退出"))?;
    let file_sep = PredefinedMenuItem::separator(app)?;

    // —— 仓库（快捷键与 Windows 前端注册的 Ctrl/⌘ 组合一致）——
    let commit_item = MenuItem::with_id(app, "commit", "提交", true, Some("CmdOrCtrl+S"))?;
    let push_item = MenuItem::with_id(app, "push", "推送", true, Some("CmdOrCtrl+Shift+P"))?;
    let pull_item = MenuItem::with_id(app, "pull", "拉取", true, Some("CmdOrCtrl+Shift+L"))?;
    let refresh_item = MenuItem::with_id(app, "refresh", "刷新", true, Some("CmdOrCtrl+R"))?;
    let repo_sep = PredefinedMenuItem::separator(app)?;

    // —— 分支 ——
    let new_branch_item = MenuItem::with_id(app, "new-branch", "新建分支", true, None::<&str>)?;
    let checkout_branch_item =
        MenuItem::with_id(app, "checkout-branch", "切换分支", true, None::<&str>)?;

    // —— 视图（与 Windows MenuBar.vue 的顺序、勾选语义完全一致）——
    // 深色主题初始 false：前端 useTheme 默认浅色；启动后 menu-state 按 localStorage 实际值同步。
    let worktree_item = CheckMenuItem::with_id(
        app,
        "show-working-tree",
        "工作视图",
        true,
        true,
        None::<&str>,
    )?;
    let log_item =
        CheckMenuItem::with_id(app, "show-log", "历史视图", true, false, None::<&str>)?;
    // 提交日志面板（勾选 = 展开）：与前端 logCollapsed 取反同步，默认展开
    let toggle_log_panel_item = CheckMenuItem::with_id(
        app,
        "toggle-log-panel",
        "提交日志面板",
        true,
        true,
        None::<&str>,
    )?;
    let toggle_submodules_item = CheckMenuItem::with_id(
        app,
        "toggle-submodules",
        "子模块",
        true,
        false,
        None::<&str>,
    )?;
    let toggle_sidebar_item =
        CheckMenuItem::with_id(app, "toggle-sidebar", "侧边栏", true, true, None::<&str>)?;
    let toggle_theme_item =
        CheckMenuItem::with_id(app, "toggle-theme", "深色主题", true, false, None::<&str>)?;
    let view_sep1 = PredefinedMenuItem::separator(app)?;
    let view_sep2 = PredefinedMenuItem::separator(app)?;

    // —— 帮助 ——
    let about_item = MenuItem::with_id(app, "about", "关于 SnapGit", true, None::<&str>)?;

    // —— 编辑：标准粘贴板项（macOS 必需）+ 配置 + 语言 ——
    let undo_item = PredefinedMenuItem::undo(app, Some("撤销"))?;
    let redo_item = PredefinedMenuItem::redo(app, Some("重做"))?;
    let cut_item = PredefinedMenuItem::cut(app, Some("剪切"))?;
    let copy_item = PredefinedMenuItem::copy(app, Some("复制"))?;
    let paste_item = PredefinedMenuItem::paste(app, Some("粘贴"))?;
    let select_all_item = PredefinedMenuItem::select_all(app, Some("全选"))?;
    let repo_config_item = MenuItem::with_id(app, "repo-config", "配置", true, None::<&str>)?;
    let edit_sep1 = PredefinedMenuItem::separator(app)?;
    let edit_sep2 = PredefinedMenuItem::separator(app)?;
    let edit_sep3 = PredefinedMenuItem::separator(app)?;

    let lang_items: Vec<CheckMenuItem<Wry>> = locales
        .iter()
        .map(|(code, label)| {
            let id = format!("set-locale-{}", code);
            CheckMenuItem::with_id(app, id.as_str(), label.as_str(), true, false, None::<&str>)
        })
        .collect::<tauri::Result<Vec<_>>>()?;

    let mut edit_entries: Vec<&dyn IsMenuItem<Wry>> = vec![
        &undo_item,
        &redo_item,
        &edit_sep1,
        &cut_item,
        &copy_item,
        &paste_item,
        &select_all_item,
        &edit_sep2,
        &repo_config_item,
        &edit_sep3,
    ];
    for item in &lang_items {
        edit_entries.push(item);
    }

    let file_menu = Submenu::with_items(
        app,
        "文件",
        true,
        &[&open_repo_item, &clone_repo_item, &file_sep, &quit_item],
    )?;
    let edit_menu = Submenu::with_items(app, "编辑", true, &edit_entries)?;
    let repo_menu = Submenu::with_items(
        app,
        "仓库",
        true,
        &[
            &commit_item,
            &push_item,
            &pull_item,
            &repo_sep,
            &refresh_item,
        ],
    )?;
    let branch_menu = Submenu::with_items(
        app,
        "分支",
        true,
        &[&new_branch_item, &checkout_branch_item],
    )?;
    let view_menu = Submenu::with_items(
        app,
        "视图",
        true,
        &[
            &worktree_item,
            &log_item,
            &view_sep1,
            &toggle_log_panel_item,
            &toggle_submodules_item,
            &toggle_sidebar_item,
            &view_sep2,
            &toggle_theme_item,
        ],
    )?;
    let help_menu = Submenu::with_items(app, "帮助", true, &[&about_item])?;

    // 顶级菜单：macOS 会自动插入应用菜单（关于、服务、隐藏、退出）
    let menu = Menu::with_items(
        app,
        &[
            &file_menu,
            &edit_menu,
            &repo_menu,
            &branch_menu,
            &view_menu,
            &help_menu,
        ],
    )?;

    // 本地化文案表：key 与前端 src/i18n/locales/*.ts 的 menu.* 一一对应。
    // 语言项自身用「该语言的自称」（简体中文 / English），不参与界面语言翻译。
    let text_items: Vec<(String, TextItem)> = vec![
        ("menu.file".into(), TextItem::Submenu(file_menu)),
        ("menu.edit".into(), TextItem::Submenu(edit_menu.clone())),
        ("menu.repository".into(), TextItem::Submenu(repo_menu)),
        ("menu.branch".into(), TextItem::Submenu(branch_menu)),
        ("menu.view".into(), TextItem::Submenu(view_menu)),
        ("menu.help".into(), TextItem::Submenu(help_menu)),
        ("menu.openRepo".into(), TextItem::Normal(open_repo_item)),
        ("menu.cloneRepo".into(), TextItem::Normal(clone_repo_item)),
        ("menu.quit".into(), TextItem::Predefined(quit_item)),
        ("menu.undo".into(), TextItem::Predefined(undo_item)),
        ("menu.redo".into(), TextItem::Predefined(redo_item)),
        ("menu.cut".into(), TextItem::Predefined(cut_item)),
        ("menu.copy".into(), TextItem::Predefined(copy_item)),
        ("menu.paste".into(), TextItem::Predefined(paste_item)),
        ("menu.selectAll".into(), TextItem::Predefined(select_all_item)),
        ("menu.config".into(), TextItem::Normal(repo_config_item)),
        ("menu.commit".into(), TextItem::Normal(commit_item)),
        ("menu.push".into(), TextItem::Normal(push_item)),
        ("menu.pull".into(), TextItem::Normal(pull_item)),
        ("menu.refresh".into(), TextItem::Normal(refresh_item)),
        ("menu.newBranch".into(), TextItem::Normal(new_branch_item)),
        (
            "menu.checkoutBranch".into(),
            TextItem::Normal(checkout_branch_item),
        ),
        ("menu.worktreeView".into(), TextItem::Check(worktree_item.clone())),
        ("menu.logView".into(), TextItem::Check(log_item.clone())),
        (
            "menu.logPanel".into(),
            TextItem::Check(toggle_log_panel_item.clone()),
        ),
        (
            "menu.submodules".into(),
            TextItem::Check(toggle_submodules_item.clone()),
        ),
        (
            "menu.sidebar".into(),
            TextItem::Check(toggle_sidebar_item.clone()),
        ),
        (
            "menu.darkTheme".into(),
            TextItem::Check(toggle_theme_item.clone()),
        ),
        ("menu.aboutSnapGit".into(), TextItem::Normal(about_item)),
    ];

    Ok((
        menu,
        NativeMenuState {
            lang_items: Mutex::new(lang_items),
            edit_menu,
            worktree_item,
            log_item,
            log_panel_item: toggle_log_panel_item,
            sidebar_item: toggle_sidebar_item,
            submodules_item: toggle_submodules_item,
            theme_item: toggle_theme_item,
            text_items,
        },
    ))
}

// text_items 与 state 需要同时持有同一个菜单项。tauri 的菜单类型是 Arc 包装、
// 实现 Clone，clone 后指向同一原生菜单项，set_text / set_checked 互相可见。

/// 按前端下发的语言列表增删「编辑 → 语言」子项。
/// 列表不变时只刷新显示名（语言自身写法不会变，但保持幂等）。
fn sync_locales(app: &App, locales: &[(String, String)]) {
    let state = app.state::<NativeMenuState>();
    let mut current = match state.lang_items.lock() {
        Ok(g) => g,
        Err(e) => e.into_inner(),
    };

    let unchanged = current.len() == locales.len()
        && current.iter().zip(locales.iter()).all(|(item, (code, _))| {
            item.id().0 == format!("set-locale-{}", code)
        });
    if unchanged {
        for (item, (_, label)) in current.iter().zip(locales.iter()) {
            let _ = item.set_text(label);
        }
        return;
    }

    // 语言项固定在编辑菜单末尾，从 LANG_START 起逐个移除再追加新的。
    while let Ok(items) = state.edit_menu.items() {
        if items.len() <= LANG_START {
            break;
        }
        if state.edit_menu.remove_at(LANG_START).is_err() {
            break;
        }
    }

    let mut next = Vec::with_capacity(locales.len());
    for (code, label) in locales {
        let id = format!("set-locale-{}", code);
        if let Ok(item) =
            CheckMenuItem::with_id(app, id.as_str(), label.as_str(), true, false, None::<&str>)
        {
            if state.edit_menu.append(&item).is_ok() {
                next.push(item);
            }
        }
    }
    *current = next;
}

/// 前端每侧状态变化（语言/主题/侧边栏/子模块/提交日志面板/当前视图）都会发一份完整快照，
/// 这里一次性把所有勾选态对齐，避免多事件之间的先后竞争。
pub(crate) fn setup_native_menu(app: &App) -> tauri::Result<()> {
    let (menu, state) = build_menu(app, &default_locales())?;
    app.set_menu(menu)?;
    app.manage(state);

    // 原生菜单点击 → 统一转发给前端 handleMenuAction。
    app.on_menu_event(|app, event| {
        let id = event.id().0.clone();
        if id.starts_with("set-locale-") {
            let state = app.state::<NativeMenuState>();
            // guard 必须是局部变量：直接 if let Ok(..) = state.lang_items.lock()
            // 会让 Result 临时值活到 state 之后，触发 E0597。
            let guard = state.lang_items.lock();
            if let Ok(items) = guard {
                for item in items.iter() {
                    let _ = item.set_checked(item.id().0 == id);
                }
            }
        }
        if let Some(win) = app.get_webview_window("main") {
            let _ = win.emit("menu-action", serde_json::json!({ "action": id }));
        }
    });

    let handle = app.clone();
    app.listen("menu-state", move |event| {
        let payload: serde_json::Value =
            serde_json::from_str(event.payload()).unwrap_or(serde_json::Value::Null);
        let app = &handle;

        if let Some(arr) = payload.get("locales").and_then(|v| v.as_array()) {
            let locales: Vec<(String, String)> = arr
                .iter()
                .filter_map(|v| {
                    Some((
                        v.get("code")?.as_str()?.to_string(),
                        v.get("label")?.as_str()?.to_string(),
                    ))
                })
                .collect();
            if !locales.is_empty() {
                sync_locales(app, &locales);
            }
        }

        if let Some(labels) = payload.get("labels").and_then(|v| v.as_object()) {
            let state = app.state::<NativeMenuState>();
            for (key, item) in &state.text_items {
                if let Some(text) = labels.get(key).and_then(|v| v.as_str()) {
                    item.set_text(text);
                }
            }
        }

        let state = app.state::<NativeMenuState>();
        if let Some(code) = payload.get("locale").and_then(|v| v.as_str()) {
            let target = format!("set-locale-{}", code);
            let guard = state.lang_items.lock();
            if let Ok(items) = guard {
                for item in items.iter() {
                    let _ = item.set_checked(item.id().0 == target);
                }
            }
        }
        if let Some(dark) = payload.get("dark").and_then(|v| v.as_bool()) {
            let _ = state.theme_item.set_checked(dark);
        }
        if let Some(collapsed) = payload.get("sidebarCollapsed").and_then(|v| v.as_bool()) {
            let _ = state.sidebar_item.set_checked(!collapsed);
        }
        if let Some(visible) = payload.get("submodulesVisible").and_then(|v| v.as_bool()) {
            let _ = state.submodules_item.set_checked(visible);
        }
        if let Some(visible) = payload.get("logPanelVisible").and_then(|v| v.as_bool()) {
            let _ = state.log_panel_item.set_checked(visible);
        }
        if let Some(view) = payload.get("view").and_then(|v| v.as_str()) {
            let _ = state.worktree_item.set_checked(view == "worktree");
            let _ = state.log_item.set_checked(view == "log");
        }
    });

    Ok(())
}
