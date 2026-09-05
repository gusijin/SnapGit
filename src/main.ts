import { createApp } from 'vue'
import './style.css'
import App from './App.vue'
import EditorWindow from './views/EditorWindow.vue'
import router from './router'
import { i18n } from './i18n'
import { getCurrentWindow } from '@tauri-apps/api/window'

const STORAGE_KEY = 'snapgit-theme'
try {
  const saved = localStorage.getItem(STORAGE_KEY)
  const theme = saved === 'dark' || saved === 'light' ? saved : 'light'
  document.documentElement.setAttribute('data-theme', theme)
} catch (e) {
  console.error('[SnapGit] 读取主题失败:', e)
}

interface EditArgs {
  repoPath: string
  filePath: string
  mode: 'conflict' | 'diff'
}

// 从 URL search / hash query 解析编辑参数，双保险。
// 优先 search：Tauri dev 模式下独立窗口的 hash 可能丢失，search 更稳。
function parseEditArgsFromUrl(): EditArgs | null {
  const searchParams = new URLSearchParams(window.location.search)
  const repoPath = searchParams.get('repoPath')
  const filePath = searchParams.get('filePath')
  const mode = searchParams.get('mode')
  if (repoPath && filePath) {
    return { repoPath, filePath, mode: mode === 'conflict' ? 'conflict' : 'diff' }
  }

  const hash = window.location.hash || ''
  const qIndex = hash.indexOf('?')
  if (qIndex >= 0) {
    const hashParams = new URLSearchParams(hash.slice(qIndex + 1))
    const rp = hashParams.get('repoPath')
    const fp = hashParams.get('filePath')
    const md = hashParams.get('mode')
    if (rp && fp) {
      return { repoPath: rp, filePath: fp, mode: md === 'conflict' ? 'conflict' : 'diff' }
    }
  }
  return null
}

// Rust 端 open_edit_window 会通过 initialization_script 注入编辑参数：
// window.__SNAPGIT_EDIT_ARGS__ = { repoPath, filePath, mode }
// 同时从 URL search/hash 读取参数作为双保险。
const scriptArgs = (window as unknown as { __SNAPGIT_EDIT_ARGS__?: EditArgs }).__SNAPGIT_EDIT_ARGS__
const urlArgs = parseEditArgsFromUrl()
const editArgs = scriptArgs || urlArgs

// 是否为「独立编辑窗口」上下文：用 Tauri 官方 API 取当前窗口 label（最可靠，
// 内部正确处理 __TAURI_INTERNALS__ 注入时机）。label 以 editor- 开头即为编辑窗口。
// 参数由 EditorWindow 自己调用 get_edit_args() 获取，绕过 initialization_script 失效问题。
function getWindowLabel(): string {
  try {
    return getCurrentWindow().label || ''
  } catch {
    const internals = (window as unknown as { __TAURI_INTERNALS__?: { metadata?: { currentWindow?: { label?: string } } } }).__TAURI_INTERNALS__
    return internals?.metadata?.currentWindow?.label || ''
  }
}
const currentWindowLabel = getWindowLabel()
const isEditorByLabel = currentWindowLabel.startsWith('editor-')
const isEditorContext = isEditorByLabel || !!editArgs

// 把最终生效的参数写回 window，方便 EditorWindow.vue 统一读取
if (editArgs) {
  ;(window as unknown as { __SNAPGIT_EDIT_ARGS__?: EditArgs }).__SNAPGIT_EDIT_ARGS__ = editArgs
}

if (isEditorContext) {
  console.log('[SnapGit] 编辑窗口上下文，label:', currentWindowLabel, 'URL:', window.location.href)
  try {
    const editorApp = createApp(EditorWindow)
    editorApp.use(i18n)
    editorApp.config.errorHandler = (err, _vm, info) => {
      console.error('[SnapGit] EditorWindow 渲染错误:', err, info)
    }
    editorApp.mount('#app')
  } catch (e) {
    console.error('[SnapGit] 编辑窗口挂载失败:', e)
  }
} else {
  console.log('[SnapGit] 主窗口上下文，URL:', window.location.href)
  // 诊断「整窗白屏后恢复」：每次页面加载都会打印本日志。
  // 若白屏发生时控制台重新出现这条加载日志 → 是整页重载（外部因素触发）；
  // 若只出现渲染错误日志而不重载 → 是组件渲染中断（Vue 层问题）。
  console.log('[SnapGit] 主窗口页面加载', new Date().toISOString())
  window.addEventListener('beforeunload', () => {
    console.warn('[SnapGit] 页面即将卸载：发生了整页重载或窗口关闭')
  })
  // 未处理的 Promise 拒绝默认只有浏览器静默记录，这里显式打印便于排查
  window.addEventListener('unhandledrejection', (e) => {
    console.error('[SnapGit] 未处理的 Promise 拒绝:', e.reason)
  })
  try {
    const app = createApp(App)
    // 全局错误捕获：避免单个组件渲染/异步错误冒泡卸载根组件，
    // 否则会造成「整个 SnapGit 变空白再显示」的整窗白屏（下一次渲染才恢复）。
    // 捕获后仅打印到控制台，主窗口保持存活、面板继续显示。
    app.config.errorHandler = (err, _vm, info) => {
      console.error('[SnapGit] 渲染/运行时未捕获错误:', err, info)
    }
    app.use(router)
    app.use(i18n)
    app.mount('#app')
  } catch (e) {
    console.error('[SnapGit] 主窗口挂载失败:', e)
  }
}
