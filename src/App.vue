<script setup lang="ts">
import { RouterView } from 'vue-router'
import { onMounted, onBeforeUnmount } from 'vue'

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

onMounted(() => {
  document.addEventListener('contextmenu', onContextMenuCapture, true)
})

onBeforeUnmount(() => {
  document.removeEventListener('contextmenu', onContextMenuCapture, true)
})
</script>

<template>
  <RouterView />
</template>

<style scoped>
</style>
