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
  // Git 检测是启动时唯一会 fork 子进程的命令（git --version）。虽然后端已改成
  // async + spawn_blocking 不再占主线程，这里仍把它挪到首屏之后：把 IPC 通道
  // 优先让给 get_initial_data，不与首屏四项数据争资源。Git 缺失时的引导弹窗
  // 晚一点出现，不影响功能（属于异常路径）。
  // 打点用于自证：若「耗时」接近 5~6 秒，即可确认冷启慢来自 git.exe 首次拉起。
  const runDetectGit = () => {
    const t0 = performance.now()
    detectGit().finally(() => {
      console.log(`[SnapGit:perf] Git 检测完成 +${Math.round(performance.now())}ms（耗时 ${Math.round(performance.now() - t0)}ms）`)
    })
  }
  const ric = (window as unknown as {
    requestIdleCallback?: (cb: () => void, opts?: { timeout: number }) => void
  }).requestIdleCallback
  if (typeof ric === 'function') {
    ric(runDetectGit, { timeout: 1500 })
  } else {
    setTimeout(runDetectGit, 0)
  }
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
