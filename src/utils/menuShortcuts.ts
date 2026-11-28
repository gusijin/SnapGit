/**
 * 应用级键盘快捷键映射（纯函数，便于单独验证）。
 *
 * macOS 的 ⌘S / ⇧⌘P / ⇧⌘L / ⌘R 由系统原生菜单的 accelerator 派发成 menu-action，
 * 前端必须放行，否则同一次按键会触发两次（提交/推送会被执行两遍）。
 * ⌘B（侧边栏）原生菜单没有对应项，仍由前端处理。
 * Windows/Linux 没有原生菜单，全部快捷键由前端注册。
 */
export type ShortcutAction = 'toggle-sidebar' | 'commit' | 'push' | 'pull' | 'refresh'

export interface ShortcutKey {
  key: string
  ctrlKey: boolean
  metaKey: boolean
  shiftKey: boolean
}

export function matchShortcut(e: ShortcutKey, isMacOS: boolean): ShortcutAction | null {
  if (!e.ctrlKey && !e.metaKey) return null

  const key = e.key.toLowerCase()
  if (key === 'b' && !e.shiftKey) return 'toggle-sidebar'
  if (isMacOS) return null

  if (e.shiftKey) {
    if (key === 'p') return 'push'
    if (key === 'l') return 'pull'
    return null
  }
  if (key === 's') return 'commit'
  if (key === 'r') return 'refresh'
  return null
}
