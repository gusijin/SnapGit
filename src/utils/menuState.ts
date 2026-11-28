import { availableLocales } from '../i18n'

/** 需要下发给 macOS 原生菜单的文案 key（对应 src/i18n/locales/*.ts 的 menu.*）。 */
export const MENU_LABEL_KEYS = [
  'file', 'edit', 'view', 'repository', 'branch', 'help',
  'openRepo', 'cloneRepo', 'quit', 'config',
  'undo', 'redo', 'cut', 'copy', 'paste', 'selectAll',
  'commit', 'push', 'pull', 'refresh', 'newBranch', 'checkoutBranch',
  'worktreeView', 'logView', 'logPanel', 'submodules', 'sidebar', 'darkTheme', 'aboutSnapGit',
] as const

export interface MenuStateInput {
  locale: string
  dark: boolean
  sidebarCollapsed: boolean
  submodulesVisible: boolean
  /** 提交日志面板是否展开（= !logCollapsed），同步 macOS「视图 → 提交日志面板」勾选态 */
  logPanelVisible: boolean
  view: 'worktree' | 'log'
  t: (key: string) => string
}

export interface MenuStatePayload {
  locales: { code: string; label: string }[]
  locale: string
  dark: boolean
  sidebarCollapsed: boolean
  submodulesVisible: boolean
  logPanelVisible: boolean
  view: string
  labels: Record<string, string>
}

/**
 * 组装发给 Rust native_menu.rs 的完整状态快照（menu-state 事件）。
 * 语言清单直接取 LocaleRegistry，Rust 侧不再需要硬编码语言表；
 * labels 按当前界面语言翻译，切换语言时原生菜单文案一起更新。
 */
export function buildMenuState(input: MenuStateInput): MenuStatePayload {
  const labels: Record<string, string> = {}
  for (const key of MENU_LABEL_KEYS) labels[`menu.${key}`] = input.t(`menu.${key}`)

  return {
    locales: availableLocales(),
    locale: input.locale,
    dark: input.dark,
    sidebarCollapsed: input.sidebarCollapsed,
    submodulesVisible: input.submodulesVisible,
    logPanelVisible: input.logPanelVisible,
    view: input.view,
    labels,
  }
}
