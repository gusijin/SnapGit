<script setup lang="ts">
import { RouterView } from 'vue-router'
import { ref, onMounted, onBeforeUnmount } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import GitRequiredDialog from '@/components/GitRequiredDialog.vue'
import ConfirmCloseDialog from '@/components/ConfirmCloseDialog.vue'
import { useGit } from '@/stores/git'

// Chrome 区域（菜单栏 / 工具栏 / 面板标题栏 / 分组与表头标题）禁用右键：
// 右键点击这些区域时不弹出任何上下文菜单。
// 使用 capture 阶段统一拦截，先于任何子组件的事件处理器执行，
// 且后续新增面板只要复用这些类名即自动生效。
function onContextMenuCapture(e: MouseEvent) {
  const target = e.target as HTMLElement | null
  if (!target || !(target instanceof HTMLElement)) return
  if (target.closest('.menubar, .toolbar, .panel-header, .list-header, .group-title, .log-header')) {
    e.preventDefault()
  }
}

const { detectGit } = useGit()

// ============ 关闭主窗口前的 Git 忙碌确认 ============
// Rust 侧拦截 CloseRequested 并 emit "close-requested"（见 main.rs on_window_event）。
// 这里查询 is_git_busy：忙 → 弹确认框；空闲 → destroy() 直接退出。
// destroy 不触发 CloseRequested，不会递归回弹窗。
const showCloseConfirm = ref(false)
let unlistenClose: (() => void) | null = null

async function forceExit() {
  try {
    await getCurrentWindow().destroy()
  } catch (e) {
    console.error('[App] force exit failed:', e)
  }
}

onMounted(() => {
  document.addEventListener('contextmenu', onContextMenuCapture, true)
  detectGit()
  // 关闭确认监听注册失败时不拦截兜底会导致窗口关不掉，因此 Rust 端已拦；
  // 这里失败仅记日志（正常桌面端不会发生）。
  listen('close-requested', async () => {
    try {
      const busy = await invoke<boolean>('is_git_busy')
      if (busy) {
        showCloseConfirm.value = true
      } else {
        await forceExit()
      }
    } catch (e) {
      console.error('[App] close-requested handling failed, exit directly:', e)
      await forceExit()
    }
  })
    .then((unlisten) => { unlistenClose = unlisten })
    .catch((e) => console.error('[App] 注册 close-requested 监听失败:', e))
})

onBeforeUnmount(() => {
  document.removeEventListener('contextmenu', onContextMenuCapture, true)
  unlistenClose?.()
})
</script>

<template>
  <RouterView />
  <GitRequiredDialog />
  <ConfirmCloseDialog v-model:open="showCloseConfirm" @force-exit="forceExit" />
</template>

<style scoped>
</style>
