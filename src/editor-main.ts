import { createApp } from 'vue'
import './style.css'
import EditorWindow from './views/EditorWindow.vue'
import { i18n } from './i18n'

const STORAGE_KEY = 'snapgit-theme'
try {
  const saved = localStorage.getItem(STORAGE_KEY)
  const theme = saved === 'dark' || saved === 'light' ? saved : 'light'
  document.documentElement.setAttribute('data-theme', theme)
} catch (e) {
  console.error('[SnapGit] 读取主题失败:', e)
}

// editor.html 专用入口：不需要 router，不需要判断窗口 label，直接挂载 EditorWindow。
// EditorWindow 内部会调用 getEditArgs() 从 Rust 取参，同时尝试读取 __SNAPGIT_EDIT_ARGS__。
console.log('[SnapGit] editor-main.ts 启动，URL:', window.location.href)

try {
  const editorApp = createApp(EditorWindow)
  editorApp.use(i18n)
  editorApp.config.errorHandler = (err, _vm, info) => {
    console.error('[SnapGit] EditorWindow 渲染错误:', err, info)
  }
  editorApp.mount('#app')
  console.log('[SnapGit] EditorWindow 挂载成功')
} catch (e) {
  console.error('[SnapGit] 编辑窗口挂载失败:', e)
}
