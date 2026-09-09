<script setup lang="ts">
import { ref, computed, watch, nextTick } from 'vue'
import { useI18n } from 'vue-i18n'
import type { FileStatus } from '../types'
// 文件类型图标映射统一从共享工具引入（与仓库面板文件树共用一套，保证风格一致）
import { getFileIcon } from '../utils/fileIcons'
import {
  CheckCircle2, Check, Plus, Trash2, RefreshCw,
} from 'lucide-vue-next'

interface Props {
  files: FileStatus[]
  selectedFiles: string[]
  viewMode: 'working-tree' | 'commit' | 'log'
}

const props = defineProps<Props>()
const emit = defineEmits(['select-files', 'stage-files', 'commit-files', 'discard-files', 'open-file', 'refresh'])
const { t } = useI18n()

const pendingClickTimer = ref<number | null>(null)
const PENDING_CLICK_DELAY = 200

const fileListRef = ref<HTMLElement | null>(null)
const rightClickFile = ref<FileStatus | null>(null)
const showContextMenu = ref(false)
const contextMenuPos = ref({ x: 0, y: 0 })
const lastSelectedIndex = ref(-1)

// 当文件列表刷新（新增/删除/重排序）时钉住滚动位置（像素），
// 避免视口内容因列表结构变化而跳动或闪烁。
// 注：曾用"锚定首行 path 并滚回顶部"方案，但列表在首行上方增删文件时，
// 会把某个文件顶到视口顶部造成明显跳动；钉住 scrollTop 像素可彻底消除该跳变。
watch(() => props.files, () => {
  const el = fileListRef.value
  if (!el) return
  const prevTop = el.scrollTop
  lastSelectedIndex.value = -1
  nextTick(() => {
    const newEl = fileListRef.value
    if (newEl) newEl.scrollTop = prevTop
  })
})

function getStatusInfo(status: string): { color: string; textKey: string | null; bg: string } {
  switch (status) {
    case 'new':
    case 'untracked':
      return { color: 'var(--color-add)', textKey: 'fileList.statusNew', bg: 'var(--bg-add)' }
    case 'modified':
      return { color: 'var(--color-mod)', textKey: 'fileList.statusModified', bg: 'var(--bg-mod)' }
    case 'deleted':
      return { color: 'var(--color-del)', textKey: 'fileList.statusDeleted', bg: 'var(--bg-del)' }
    case 'conflict':
    case 'unmerged':
      return { color: 'var(--color-del)', textKey: 'fileList.statusConflict', bg: 'var(--bg-del)' }
    case 'renamed':
      return { color: 'var(--accent-primary)', textKey: 'fileList.statusRenamed', bg: 'var(--brand-bg)' }
    default:
      return { color: 'var(--text-tertiary)', textKey: null, bg: 'var(--bg-tertiary)' }
  }
}

function getFileName(path: string): string {
  const parts = path.replace(/\\/g, '/').split('/')
  return parts[parts.length - 1] || path
}

function getFilePathDir(path: string): string {
  const parts = path.replace(/\\/g, '/').split('/')
  parts.pop()
  return parts.join('/') || '.'
}

function isSelected(path: string): boolean {
  return props.selectedFiles.includes(path)
}

function scheduleSelectFile(file: FileStatus, event: MouseEvent) {
  if (pendingClickTimer.value !== null) {
    window.clearTimeout(pendingClickTimer.value)
    pendingClickTimer.value = null
  }
  pendingClickTimer.value = window.setTimeout(() => {
    pendingClickTimer.value = null
    executeSelectFile(file, event)
  }, PENDING_CLICK_DELAY)
}

function executeSelectFile(file: FileStatus, event: MouseEvent) {
  const files = sortedFiles.value
  const currentIndex = files.findIndex(f => f.path === file.path)
  const currentSelected = isSelected(file.path)
  const isShift = event.shiftKey
  const isCtrl = event.ctrlKey || event.metaKey

  let newSelected: string[]

  if (isShift && lastSelectedIndex.value >= 0) {
    // Shift+click: range selection from last selected to current
    const start = Math.min(lastSelectedIndex.value, currentIndex)
    const end = Math.max(lastSelectedIndex.value, currentIndex)
    newSelected = files.slice(start, end + 1).map(f => f.path)
  } else if (isCtrl || event.metaKey) {
    // Ctrl/Cmd+click: toggle single file
    newSelected = [...props.selectedFiles]
    if (currentSelected) {
      newSelected = newSelected.filter(p => p !== file.path)
    } else {
      newSelected.push(file.path)
    }
    lastSelectedIndex.value = currentIndex
  } else if (currentSelected) {
    // Click on already selected file: deselect it
    newSelected = props.selectedFiles.filter(p => p !== file.path)
    lastSelectedIndex.value = currentIndex
  } else {
    // Normal click on unselected file: select only this file
    newSelected = [file.path]
    lastSelectedIndex.value = currentIndex
  }

  emit('select-files', newSelected)
  fileListRef.value?.focus()
  closeContextMenu()
}

function handleOpenFile(file: FileStatus, event: MouseEvent) {
  // 双击时取消待处理的单击选择，避免先选中再取消的闪烁
  if (pendingClickTimer.value !== null) {
    window.clearTimeout(pendingClickTimer.value)
    pendingClickTimer.value = null
  }
  // 提交历史文件：仅单击查看差异，双击不打开编辑（降级为选中并显示差异）
  // 变更文件（工作区）：双击才打开编辑窗口
  if (props.viewMode === 'working-tree') {
    emit('open-file', file)
  } else {
    executeSelectFile(file, event)
  }
  closeContextMenu()
}

function handleRightClick(e: MouseEvent, file: FileStatus) {
  e.preventDefault()
  // 阻止冒泡到根元素的空白右键处理（否则会覆盖成"无目标文件"菜单）
  e.stopPropagation()
  // Right-click on an unselected file: select only that file first
  if (!isSelected(file.path)) {
    emit('select-files', [file.path])
    lastSelectedIndex.value = sortedFiles.value.findIndex(f => f.path === file.path)
  }
  rightClickFile.value = file
  showContextMenu.value = true
  contextMenuPos.value = { x: e.clientX, y: e.clientY }
}

/** 空白处右键：无目标文件，菜单只含「刷新」 */
function handleBlankRightClick(e: MouseEvent) {
  e.preventDefault()
  rightClickFile.value = null
  showContextMenu.value = true
  contextMenuPos.value = { x: e.clientX, y: e.clientY }
}

function handleRefresh() {
  emit('refresh')
  closeContextMenu()
}

function closeContextMenu() {
  showContextMenu.value = false
  rightClickFile.value = null
}

function handleStage() {
  emit('stage-files', props.selectedFiles)
  closeContextMenu()
}

function handleCommitFile() {
  emit('commit-files', props.selectedFiles)
  closeContextMenu()
}

function handleDiscard() {
  emit('discard-files', props.selectedFiles)
  closeContextMenu()
}

const sortedFiles = computed(() => {
  void props.viewMode
  return [...props.files].sort((a, b) => {
    const nameA = getFileName(a.path).toLowerCase()
    const nameB = getFileName(b.path).toLowerCase()
    return nameA.localeCompare(nameB)
  })
})

function selectAllFiles() {
  if (sortedFiles.value.length === 0) return
  emit('select-files', sortedFiles.value.map(f => f.path))
}

const isMultiSelected = computed(() => props.selectedFiles.length > 1)
</script>

<template>
  <div
    class="file-list"
    ref="fileListRef"
    tabindex="-1"
    @click.self="closeContextMenu"
    @contextmenu="handleBlankRightClick"
    @keydown.ctrl.a.prevent="selectAllFiles"
    @keydown.meta.a.prevent="selectAllFiles"
  >
    <div class="panel-header">
      <span v-if="viewMode === 'working-tree'">{{ t('fileList.changedFiles') }}</span>
      <span v-else>{{ t('fileList.commitFiles') }}</span>
      <span class="count" v-if="files.length > 0">({{ files.length }})</span>
      <span class="count" v-else>(0)</span>
      <span class="selected-count" v-if="selectedFiles.length > 0">{{ t('fileList.selected', { n: selectedFiles.length }) }}</span>
    </div>

    <div v-if="sortedFiles.length === 0" class="empty">
      <CheckCircle2 :size="44" class="empty-icon" />
      <p v-if="viewMode === 'working-tree'">{{ t('fileList.wsClean') }}</p>
      <p v-else>{{ t('fileList.noFileChanges') }}</p>
    </div>

    <div v-else class="file-list-content">
      <div
        v-for="file in sortedFiles"
        :key="file.path"
        :data-path="file.path"
        class="file-item"
        :class="{ selected: isSelected(file.path), conflict: file.status === 'conflict' || file.status === 'unmerged' }"
        @click="scheduleSelectFile(file, $event)"
        @dblclick="handleOpenFile(file, $event)"
        @contextmenu="handleRightClick($event, file)"
      >
        <component :is="getFileIcon(getFileName(file.path))" :size="16" class="file-icon" />
        <div class="file-info">
          <span class="file-name">{{ getFileName(file.path) }}</span>
          <span :style="{ color: getStatusInfo(file.status).color }" class="file-status">
          <template v-if="getStatusInfo(file.status).textKey">{{ t(getStatusInfo(file.status).textKey!) }}</template>
          <template v-else>{{ file.status }}</template>
        </span>
          <span class="file-path">{{ getFilePathDir(file.path) }}</span>
        </div>
      </div>
    </div>

    <Teleport to="body">
      <div
        v-if="showContextMenu"
        class="context-menu"
        :style="{ left: contextMenuPos.x + 'px', top: contextMenuPos.y + 'px' }"
        @click.stop
      >
        <div class="menu-item" @click="handleCommitFile" v-if="rightClickFile && viewMode === 'working-tree'">
          <Check :size="14" class="menu-icon" />
          <span v-if="isMultiSelected">{{ t('fileList.commitSelected') }}</span>
          <span v-else>{{ t('fileList.commitThis') }}</span>
        </div>
        <div class="menu-item" @click="handleStage" v-if="rightClickFile && viewMode === 'working-tree'">
          <Plus :size="14" class="menu-icon" />
          <span v-if="isMultiSelected">{{ t('fileList.stageSelected') }}</span>
          <span v-else>{{ t('fileList.stageThis') }}</span>
        </div>
        <div class="menu-item danger" @click="handleDiscard" v-if="rightClickFile && viewMode === 'working-tree'">
          <Trash2 :size="14" class="menu-icon" />
          <span v-if="isMultiSelected">{{ t('fileList.discardSelected') }}</span>
          <span v-else>{{ t('fileList.discardThis') }}</span>
        </div>
        <div v-if="rightClickFile && viewMode === 'working-tree'" class="menu-divider"></div>
        <div class="menu-item" @click="handleRefresh">
          <RefreshCw :size="14" class="menu-icon" />
          <span>{{ t('fileList.refresh') }}</span>
        </div>
      </div>
      <div
        v-if="showContextMenu"
        class="context-mask"
        @click="closeContextMenu"
        @contextmenu.prevent="closeContextMenu"
      ></div>
    </Teleport>
  </div>
</template>

<style scoped>
.file-list {
  display: flex;
  flex-direction: column;
  height: 100%;
  background-color: var(--bg-secondary);
  border-right: 1px solid var(--border-color);
  min-height: 0;
  -webkit-user-select: none;
  -webkit-user-drag: none;
  user-select: none;
  outline: none;
}

.panel-header {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 10px;
  background-color: var(--bg-toolbar);
  border-bottom: 1px solid var(--border-color);
  font-size: 11px;
  color: var(--text-secondary);
  font-weight: 600;
  text-transform: uppercase;
  flex-shrink: 0;
}

.count {
  font-size: 10px;
  padding: 1px 6px;
  background-color: var(--bg-tertiary);
  border-radius: 8px;
  color: var(--text-tertiary);
}

.selected-count {
  font-size: 10px;
  padding: 1px 6px;
  background-color: var(--accent-bg);
  border-radius: 8px;
  color: var(--accent-text);
  margin-left: auto;
}

.file-list-content {
  flex: 1;
  overflow: auto;
  min-height: 0;
}

.file-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 5px 10px;
  cursor: pointer;
  transition: background-color 0.1s;
}

.file-item:hover {
  background-color: var(--bg-hover);
}

.file-item.selected {
  background-color: var(--bg-selected);
}

.file-item.conflict .file-name {
  color: var(--color-del);
}

.file-icon {
  flex-shrink: 0;
  color: var(--text-tertiary);
}

.file-info {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 70px minmax(0, 1fr);
  gap: 0 12px;
  min-width: 0;
  align-items: center;
  flex: 1;
}

.file-name {
  font-size: 12px;
  color: var(--text-primary);
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.file-status {
  font-size: 10px;
  font-weight: 600;
  font-family: Consolas, Monaco, monospace;
  text-align: left;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.file-path {
  font-size: 10px;
  color: var(--text-tertiary);
  font-family: Consolas, Monaco, monospace;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.empty {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  color: var(--text-muted);
  padding: 20px;
  min-height: 0;
}

.empty-icon {
  color: var(--text-muted);
  opacity: 0.55;
}

.empty p {
  font-size: 13px;
  margin: 0;
}

/* 右键菜单 */
.context-menu {
  position: fixed;
  z-index: 9999;
  background-color: var(--bg-secondary);
  border: 1px solid var(--border-medium);
  border-radius: 6px;
  box-shadow: var(--shadow-dialog);
  padding: 4px 0;
  min-width: 160px;
  animation: fadeIn 0.1s ease-out;
}

@keyframes fadeIn {
  from { opacity: 0; transform: translateY(-4px); }
  to { opacity: 1; transform: translateY(0); }
}

.context-mask {
  position: fixed;
  inset: 0;
  z-index: 9998;
}

.menu-divider {
  height: 1px;
  margin: 4px 0;
  background-color: var(--border-light);
}

.menu-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 7px 16px;
  cursor: pointer;
  font-size: 12px;
  color: var(--text-secondary);
  transition: background-color 0.1s;
}

.menu-item:hover {
  background-color: var(--bg-active);
  color: var(--text-bright);
}

.menu-item.danger {
  color: var(--danger-color, #f44747);
}

.menu-item.danger:hover {
  background-color: var(--danger-bg, rgba(244, 71, 71, 0.15));
  color: var(--danger-color, #f44747);
}

.menu-icon {
  flex-shrink: 0;
  color: var(--text-tertiary);
}

.menu-item.danger .menu-icon {
  color: var(--danger-color, #f44747);
}
</style>
