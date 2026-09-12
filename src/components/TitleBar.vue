<script setup lang="ts">
import { ref, onMounted, onBeforeUnmount } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { Code2, Minus, Square, X, Copy } from 'lucide-vue-next'

interface Props {
  /** 副标题，显示在拖拽区左侧（可选，如当前仓库名 / 文件名） */
  subtitle?: string
}

defineProps<Props>()

// macOS 系统保留 traffic lights，标题栏不显示自定义控件
const isMacOS = /mac/i.test(navigator.platform) || /mac|darwin/i.test(navigator.userAgent)

// 当前窗口是否处于最大化状态（用于切换「最大化 / 还原」图标）
const isMaximized = ref(false)
let unlistenResized: (() => void) | null = null

async function syncMaximized() {
  try {
    isMaximized.value = await getCurrentWindow().isMaximized()
  } catch {
    // 忽略：仅用于 UI 表现，失败时图标可能不精确
  }
}

onMounted(async () => {
  if (isMacOS) return
  await syncMaximized()
  // 监听窗口尺寸变化（maximize / unmaximize 都会触发），切换图标
  unlistenResized = await getCurrentWindow().onResized(syncMaximized)
})

onBeforeUnmount(() => {
  unlistenResized?.()
})

async function minimize() {
  try {
    await getCurrentWindow().minimize()
  } catch (e) {
    console.error('[TitleBar] minimize failed:', e)
  }
}

async function toggleMaximize() {
  try {
    await getCurrentWindow().toggleMaximize()
  } catch (e) {
    console.error('[TitleBar] toggleMaximize failed:', e)
  }
}

async function closeWindow() {
  try {
    await getCurrentWindow().close()
  } catch (e) {
    console.error('[TitleBar] close failed:', e)
  }
}
</script>

<template>
  <!-- macOS：使用原生透明标题栏（Transparent 样式），交通灯由系统渲染、
       且标题栏本身可拖拽，无需前端占位元素；应用内容直接排在原生标题栏下方。 -->

  <!-- Windows / Linux：自定义标题栏，跟随主题（颜色全部用 CSS 变量） -->
  <div v-if="!isMacOS" class="titlebar">
    <!-- 左侧：app 图标 + 应用名（可拖拽） -->
    <div class="titlebar-left" data-tauri-drag-region>
      <Code2 :size="14" class="titlebar-logo" />
      <span class="titlebar-title">SnapGit</span>
      <span v-if="subtitle" class="titlebar-subtitle">— {{ subtitle }}</span>
    </div>

    <!-- 中部：拖拽区（让双击最大化生效） -->
    <div class="titlebar-drag" data-tauri-drag-region></div>

    <!-- 右侧：Windows 三按钮 -->
    <div class="titlebar-controls">
      <button
        class="titlebar-btn"
        title="最小化"
        aria-label="最小化"
        @click="minimize"
      >
        <Minus :size="14" />
      </button>
      <button
        class="titlebar-btn"
        :title="isMaximized ? '还原' : '最大化'"
        :aria-label="isMaximized ? '还原' : '最大化'"
        @click="toggleMaximize"
      >
        <component :is="isMaximized ? Copy : Square" :size="12" />
      </button>
      <button
        class="titlebar-btn titlebar-btn-close"
        title="关闭"
        aria-label="关闭"
        @click="closeWindow"
      >
        <X :size="14" />
      </button>
    </div>
  </div>
</template>

<style scoped>
/* ============ 自定义标题栏（Windows / Linux） ============ */
.titlebar {
  display: flex;
  align-items: center;
  height: 32px;
  flex-shrink: 0;
  background-color: var(--bg-tertiary);
  border-bottom: 1px solid var(--border-color);
  user-select: none;
  -webkit-user-select: none;
  -webkit-app-region: drag;
}

/* 左侧：logo + 应用名 */
.titlebar-left {
  display: flex;
  align-items: center;
  gap: 6px;
  padding-left: 10px;
  font-size: 12px;
  color: var(--text-secondary);
  white-space: nowrap;
}

.titlebar-logo {
  color: var(--accent-primary);
  flex-shrink: 0;
}

.titlebar-title {
  font-weight: 600;
  color: var(--text-primary);
}

.titlebar-subtitle {
  color: var(--text-muted, #94a3b8);
  margin-left: 4px;
  font-weight: normal;
}

/* 中部：纯拖拽区，撑满剩余空间让双击最大化生效 */
.titlebar-drag {
  flex: 1;
  height: 100%;
  /* data-tauri-drag-region 已生效，flex 即可撑满 */
}

/* 右侧：三按钮 */
.titlebar-controls {
  display: flex;
  -webkit-app-region: no-drag;
}

.titlebar-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 46px;
  height: 32px;
  border: 0;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  transition: background-color 0.12s ease, color 0.12s ease;
}

.titlebar-btn:hover {
  background-color: var(--bg-hover);
  color: var(--text-primary);
}

/* 关闭按钮 hover：红底白字（Windows 风格） */
.titlebar-btn-close:hover {
  background-color: #dc2626;
  color: #ffffff;
}

.titlebar-btn:active {
  background-color: var(--bg-active, rgba(0, 0, 0, 0.08));
}

.titlebar-btn-close:active {
  background-color: #b91c1c;
}
</style>