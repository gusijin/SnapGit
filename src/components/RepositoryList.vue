<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { Folder, FolderOpen, Loader2, Trash2, ChevronRight, ChevronDown, Settings } from 'lucide-vue-next'
import type { ScannedProject, FileTreeNode, FileStatus } from '../types'
// 文件类型图标映射与变更文件面板 FileList 共用同一套，保证风格统一
import { getFileIcon } from '../utils/fileIcons'

interface Props {
  projects: ScannedProject[]
  currentPath: string
  isScanning: boolean
  fileTrees: Record<string, FileTreeNode[]>
  fileStatuses: FileStatus[]
  loadingFileTree: string | null
  // 切换中指示：switchingPath 为正在切换的仓库路径，switchingShown 为是否已超过 1 秒
  switchingPath: string | null
  switchingShown: boolean
}

const props = defineProps<Props>()
const emit = defineEmits(['select-project', 'delete-project', 'config-project', 'select-file', 'load-file-tree'])
const { t } = useI18n()

const expandedPaths = ref<Set<string>>(new Set())
const expandedFileNodes = ref<Set<string>>(new Set())

const fileStatusMap = computed(() => {
  const map = new Map<string, string>()
  props.fileStatuses.forEach(fs => map.set(fs.path, fs.status))
  return map
})

// 自定义轻量 Hint（与变更文件面板 FileList 同款风格）：鼠标跟随显示完整仓库路径
const hintVisible = ref(false)
const hintPos = ref({ x: 0, y: 0 })
const hintText = ref('')
let hideHintTimer: number | null = null

function showHint(project: ScannedProject, event: MouseEvent) {
  if (hideHintTimer) { clearTimeout(hideHintTimer); hideHintTimer = null }
  // 兜底：旧缓存里可能存有 \\?\ 前缀的扩展路径，显示前剥掉
  hintText.value = project.path.replace(/^\\\\\?\\/, '')
  hintPos.value = { x: event.clientX + 12, y: event.clientY + 12 }
  hintVisible.value = true
}

function moveHint(event: MouseEvent) {
  if (!hintVisible.value) return
  hintPos.value = { x: event.clientX + 12, y: event.clientY + 12 }
}

function hideHint() {
  if (hideHintTimer) clearTimeout(hideHintTimer)
  hideHintTimer = window.setTimeout(() => {
    hintVisible.value = false
  }, 100)
}

function isProjectExpanded(path: string): boolean {
  return expandedPaths.value.has(path)
}

function toggleProjectExpand(project: ScannedProject) {
  const path = project.path
  const newSet = new Set(expandedPaths.value)
  if (newSet.has(path)) {
    newSet.delete(path)
  } else {
    newSet.add(path)
    // 请求加载文件树
    if (!props.fileTrees[path]) {
      emit('load-file-tree', path)
    }
  }
  expandedPaths.value = newSet
}

function selectProject(project: ScannedProject) {
  emit('select-project', project)
}

function isActive(path: string): boolean {
  return path === props.currentPath
}

// 该仓库是否正在切换且已超过 1 秒（此时项目图标显示为"切换中"）
function isSwitching(project: ScannedProject): boolean {
  return props.switchingPath === project.path && props.switchingShown
}

// 文件树相关
function getFileTree(projectPath: string): FileTreeNode[] {
  return props.fileTrees[projectPath] || []
}

function toggleFileNodeExpand(path: string) {
  const newSet = new Set(expandedFileNodes.value)
  if (newSet.has(path)) {
    newSet.delete(path)
  } else {
    newSet.add(path)
  }
  expandedFileNodes.value = newSet
}

function isFileNodeExpanded(path: string): boolean {
  return expandedFileNodes.value.has(path)
}

interface FlatNode {
  node: FileTreeNode
  depth: number
}

function getFlatNodes(projectPath: string): FlatNode[] {
  const fileTree = getFileTree(projectPath)
  const result: FlatNode[] = []

  function walk(nodes: FileTreeNode[], depth: number) {
    const sorted = [...nodes].sort((a, b) => {
      if (a.is_dir && !b.is_dir) return -1
      if (!a.is_dir && b.is_dir) return 1
      return a.name.localeCompare(b.name)
    })

    for (const node of sorted) {
      result.push({ node, depth })
      if (node.is_dir && expandedFileNodes.value.has(node.path) && node.children) {
        walk(node.children, depth + 1)
      }
    }
  }

  walk(fileTree, 0)
  return result
}

function getFileStatusColor(status: string | undefined): string {
  switch (status) {
    case 'new': return '#4ec9b0'
    case 'untracked': return '#228b22'
    case 'modified': return '#e2c08d'
    case 'deleted': return '#f44747'
    case 'renamed': return '#c586c0'
    default: return 'transparent'
  }
}

function selectFile(node: FileTreeNode) {
  if (!node.is_dir) {
    emit('select-file', node.path)
  }
}

// 右键菜单
const contextMenuVisible = ref(false)
const contextMenuX = ref(0)
const contextMenuY = ref(0)
const contextMenuProject = ref<ScannedProject | null>(null)

function handleContextMenu(event: MouseEvent, project: ScannedProject) {
  event.preventDefault()
  contextMenuProject.value = project
  contextMenuX.value = event.clientX
  contextMenuY.value = event.clientY
  contextMenuVisible.value = true
}

function closeContextMenu() {
  contextMenuVisible.value = false
  contextMenuProject.value = null
}

function handleOpenProject() {
  if (contextMenuProject.value) {
    selectProject(contextMenuProject.value)
  }
  closeContextMenu()
}

function handleDeleteProject() {
  if (contextMenuProject.value) {
    emit('delete-project', contextMenuProject.value)
  }
  closeContextMenu()
}

function handleConfigProject() {
  if (contextMenuProject.value) {
    emit('config-project', contextMenuProject.value)
  }
  closeContextMenu()
}

function handleClickOutside(event: MouseEvent) {
  const target = event.target as HTMLElement
  // 点击落在右键菜单内部时不关闭（菜单项自身会关闭）；点击软件任何其它地方（含其它仓库项、空白、其它面板）都关闭
  if (!target.closest('.context-menu')) {
    closeContextMenu()
  }
}

onMounted(() => {
  // 仅监听左键 click：软件任意位置左键点击即隐藏右键菜单。
  // 不监听 contextmenu——否则右键打开菜单的同一事件会冒泡到 document 把刚打开的菜单关掉。
  document.addEventListener('click', handleClickOutside)
})

onUnmounted(() => {
  document.removeEventListener('click', handleClickOutside)
})
</script>

<template>
  <!-- @contextmenu.prevent：空白区域右键不弹出任何菜单（仅仓库列表项可右键） -->
  <div class="repository-list" @contextmenu.prevent>
    <div class="list-header">
      <span class="header-title">{{ t('repositoryList.title') }}</span>
      <span class="count-badge" v-if="projects.length > 0">{{ projects.length }}</span>
    </div>
    
    <div class="list-content">
      <div v-if="isScanning" class="loading">
        <div class="spinner"></div>
        <span>{{ t('repositoryList.scanning') }}</span>
      </div>
      
      <template v-else-if="projects.length > 0">
        <template v-for="project in projects" :key="project.path">
          <div
            class="project-item"
            :class="{ active: isActive(project.path) }"
            @dblclick="selectProject(project)"
            @contextmenu="handleContextMenu($event, project)"
            @mouseenter="showHint(project, $event)"
            @mousemove="moveHint($event)"
            @mouseleave="hideHint"
          >
            <span
              class="expand-icon"
              @click.stop="toggleProjectExpand(project)"
            >
              <component
                :is="isProjectExpanded(project.path) ? ChevronDown : ChevronRight"
                :size="12"
              />
            </span>
            <Folder v-if="!isSwitching(project)" class="project-icon" :size="14" />
            <Loader2 v-else class="project-icon switching-spinner" :size="14" />
            <span class="project-name">{{ project.name }}</span>
            <span 
              v-if="project.current_branch && project.current_branch !== 'unknown'" 
              class="branch-name"
            >
              {{ project.current_branch }}
            </span>
          </div>

          <!-- 展开的文件树 -->
          <div 
            v-if="isProjectExpanded(project.path)" 
            class="project-file-tree"
          >
            <div v-if="loadingFileTree === project.path" class="tree-loading">
              <div class="mini-spinner"></div>
              <span>{{ t('repositoryList.loading') }}</span>
            </div>
            <template v-else-if="getFileTree(project.path).length > 0">
              <div
                v-for="item in getFlatNodes(project.path)"
                :key="item.node.path"
                class="tree-node"
                :style="{ paddingLeft: (item.depth * 16 + 28) + 'px' }"
              >
                <template v-if="item.node.is_dir">
                  <span class="file-expand-icon" @click.stop="toggleFileNodeExpand(item.node.path)">
                    <component
                      :is="isFileNodeExpanded(item.node.path) ? ChevronDown : ChevronRight"
                      :size="11"
                    />
                  </span>
                  <span class="dir-icon-small" @click.stop="toggleFileNodeExpand(item.node.path)">
                    <component
                      :is="isFileNodeExpanded(item.node.path) ? FolderOpen : Folder"
                      :size="12"
                    />
                  </span>
                  <span class="node-name dir-name" @click.stop="toggleFileNodeExpand(item.node.path)">
                    {{ item.node.name }}
                  </span>
                </template>
                <template v-else>
                  <span class="file-expand-placeholder"></span>
                  <component
                    :is="getFileIcon(item.node.name)"
                    :size="12"
                    class="file-icon-small"
                  />
                  <span
                    class="node-name file-name"
                    :style="{ backgroundColor: getFileStatusColor(fileStatusMap.get(item.node.path)) ? getFileStatusColor(fileStatusMap.get(item.node.path)) + '33' : 'transparent' }"
                    @click="selectFile(item.node)"
                  >
                    {{ item.node.name }}
                    <span
                      v-if="fileStatusMap.get(item.node.path)"
                      class="status-dot"
                      :style="{ backgroundColor: getFileStatusColor(fileStatusMap.get(item.node.path)) }"
                    ></span>
                  </span>
                </template>
              </div>
            </template>
            <div v-else class="tree-empty">
              <span>{{ t('repositoryList.noFiles') }}</span>
            </div>
          </div>
        </template>
      </template>
      
      <div v-else class="empty">
        <span>{{ t('repositoryList.noProjects') }}</span>
      </div>
    </div>

    <!-- 右键菜单 -->
    <Teleport to="body">
      <div
        v-if="contextMenuVisible"
        class="context-menu"
        :style="{ left: contextMenuX + 'px', top: contextMenuY + 'px' }"
      >
        <div class="context-menu-item" @click="handleOpenProject">
          <FolderOpen class="menu-icon" :size="14" />
          <span>{{ t('repositoryList.open') }}</span>
        </div>
        <div class="context-menu-item" @click="handleConfigProject">
          <Settings class="menu-icon" :size="14" />
          <span>{{ t('repositoryList.config') }}</span>
        </div>
        <div class="context-menu-divider"></div>
        <div class="context-menu-item danger" @click="handleDeleteProject">
          <Trash2 class="menu-icon" :size="14" />
          <span>{{ t('repositoryList.delete') }}</span>
        </div>
      </div>
    </Teleport>

    <!-- 自定义轻量 Hint：与变更文件面板 FileList 同款风格，鼠标跟随显示完整仓库路径 -->
    <Teleport to="body">
      <div
        v-if="hintVisible"
        class="file-hint"
        :style="{ left: hintPos.x + 'px', top: hintPos.y + 'px' }"
      >
        {{ hintText }}
      </div>
    </Teleport>
  </div>
</template>

<style scoped>
.repository-list {
  display: flex;
  flex-direction: column;
  background-color: var(--bg-secondary);
  min-height: 0;
  overflow: hidden;
  -webkit-user-select: none;
  -webkit-user-drag: none;
  user-select: none;
}

.list-header {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 10px;
  background-color: var(--bg-toolbar);
  border-bottom: 1px solid var(--border-color);
  font-size: 11px;
  color: var(--text-secondary);
  font-weight: 600;
  flex-shrink: 0;
}

.header-title {
  flex: 1;
  text-transform: uppercase;
  letter-spacing: 0.5px;
}

.count-badge {
  font-size: 10px;
  padding: 1px 6px;
  background-color: var(--bg-active);
  color: var(--text-bright);
  border-radius: 8px;
  font-weight: 600;
}

.list-content {
  flex: 1;
  overflow-y: auto;
  overflow-x: hidden;
  padding: 0;
}

.loading {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 16px;
  gap: 8px;
  font-size: 12px;
  color: var(--text-tertiary);
}

.spinner {
  width: 16px;
  height: 16px;
  border: 2px solid var(--border-medium);
  border-top-color: var(--accent-text);
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}

@keyframes spin {
  to { transform: rotate(360deg); }
}

.project-item {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 8px;
  cursor: pointer;
  transition: background-color 0.15s;
  font-size: 12px;
}

.project-item:hover {
  background-color: var(--bg-hover);
}

.project-item.active {
  background-color: var(--bg-active);
}

.project-item.active .project-name {
  color: var(--text-bright);
}

/* 展开箭头：lucide Chevron 组件，flex 居中对齐（不再依赖 emoji 的 font-size） */
.expand-icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 14px;
  height: 14px;
  color: var(--text-tertiary);
  flex-shrink: 0;
  cursor: pointer;
  transition: color 0.12s ease;
}

.expand-icon:hover {
  color: var(--text-primary);
}

.project-icon {
  flex-shrink: 0;
  color: var(--text-tertiary);
}

.switching-spinner {
  color: var(--accent-text);
  animation: spin 0.8s linear infinite;
}

/* 自定义轻量 Hint：与变更文件面板 FileList 的 .file-hint 同款风格，
   长路径加 max-width + 换行防止溢出屏幕 */
.file-hint {
  position: fixed;
  z-index: 9999;
  padding: 3px 8px;
  background: var(--bg-secondary);
  color: var(--text-secondary);
  border: 1px solid var(--border-color);
  border-radius: 4px;
  font-size: 11px;
  line-height: 1.5;
  pointer-events: none;
  max-width: 360px;
  word-break: break-all;
  box-shadow: 0 2px 6px rgba(0, 0, 0, 0.15);
  opacity: 0;
  animation: hintIn 0.1s ease forwards;
}

@keyframes hintIn {
  from { opacity: 0; }
  to { opacity: 1; }
}

.project-name {
  flex: 1;
  color: var(--text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  font-weight: 500;
  min-width: 0;
}

.branch-name {
  font-size: 10px;
  color: var(--text-tertiary);
  padding: 2px 6px;
  background-color: var(--bg-tertiary);
  border-radius: 3px;
  white-space: nowrap;
  flex-shrink: 0;
  max-width: 80px;
  overflow: hidden;
  text-overflow: ellipsis;
}

.project-item.active .branch-name {
  background-color: rgba(255, 255, 255, 0.15);
  color: var(--text-bright);
}

/* 项目内联文件树 */
.project-file-tree {
  background-color: var(--bg-primary);
  border-top: 1px solid var(--border-light);
  border-bottom: 1px solid var(--border-light);
}

.tree-loading {
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 12px;
  gap: 8px;
  font-size: 11px;
  color: var(--text-tertiary);
}

.mini-spinner {
  width: 12px;
  height: 12px;
  border: 1.5px solid var(--border-medium);
  border-top-color: var(--accent-text);
  border-radius: 50%;
  animation: spin 0.6s linear infinite;
}

.tree-node {
  display: flex;
  align-items: center;
  gap: 2px;
  cursor: pointer;
  white-space: nowrap;
  line-height: 20px;
  padding: 1px 8px 1px 0;
  font-size: 11px;
}

.tree-node:hover {
  background-color: var(--bg-hover);
}

/* 文件树图标：全部使用 lucide 线性图标，颜色统一走 --text-tertiary，
   与变更文件面板 FileList.vue 的 .file-icon 保持像素级一致（双主题自适应） */
.file-expand-icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 12px;
  height: 12px;
  color: var(--text-tertiary);
  flex-shrink: 0;
  cursor: pointer;
  transition: color 0.12s ease;
}

.file-expand-icon:hover {
  color: var(--text-primary);
}

.file-expand-placeholder {
  width: 12px;
  flex-shrink: 0;
}

.dir-icon-small,
.file-icon-small {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 12px;
  height: 12px;
  flex-shrink: 0;
  color: var(--text-tertiary);
}

/* 目录图标 hover 稍作提亮，与文件名 hover 反馈保持一致 */
.tree-node:hover .dir-icon-small {
  color: var(--text-primary);
}

.node-name {
  flex: 1;
  color: var(--text-secondary);
  display: flex;
  align-items: center;
  gap: 5px;
  padding: 0 2px;
  border-radius: 2px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.dir-name {
  font-weight: 500;
}

.file-name:hover {
  color: var(--text-bright);
}

.status-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  flex-shrink: 0;
}

.tree-empty {
  padding: 10px 16px;
  font-size: 11px;
  color: var(--text-muted);
  text-align: center;
}

.empty {
  padding: 20px;
  text-align: center;
  color: var(--text-muted);
  font-size: 12px;
}

/* 右键菜单样式（尺寸与 FileList 右键菜单保持一致） */
.context-menu {
  position: fixed;
  z-index: 9999;
  background-color: var(--bg-secondary);
  border: 1px solid var(--border-medium);
  border-radius: 6px;
  box-shadow: var(--shadow-dialog);
  padding: 4px 0;
  min-width: 160px;
  -webkit-user-select: none;
  -webkit-user-drag: none;
  user-select: none;
  animation: fadeIn 0.1s ease-out;
}

@keyframes fadeIn {
  from { opacity: 0; transform: translateY(-4px); }
  to { opacity: 1; transform: translateY(0); }
}

.context-menu-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 7px 16px;
  cursor: pointer;
  font-size: 12px;
  color: var(--text-secondary);
  transition: background-color 0.1s;
}

.context-menu-item:hover {
  background-color: var(--bg-active);
  color: var(--text-bright);
}

.context-menu-item .menu-icon {
  flex-shrink: 0;
  color: var(--text-tertiary);
}

.context-menu-item.danger {
  color: var(--danger-color, #f44747);
}

.context-menu-item.danger .menu-icon {
  color: var(--danger-color, #f44747);
}

.context-menu-item.danger:hover {
  background-color: var(--danger-bg, rgba(244, 71, 71, 0.15));
  color: var(--danger-color, #f44747);
}

.context-menu-divider {
  height: 1px;
  background-color: var(--border-light);
  margin: 4px 0;
}
</style>
