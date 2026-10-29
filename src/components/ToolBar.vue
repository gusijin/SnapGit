<script setup lang="ts">
import { ref, computed, watch, onMounted, onBeforeUnmount } from 'vue'
import type { Component } from 'vue'
import { useI18n } from 'vue-i18n'
import type { Branch } from '../types'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip'
import {
  ArrowDownToLine,
  ArrowUpFromLine,
  GitCommitVertical,
  RefreshCw,
  Archive,
  History,
  GitBranch,
  Check,
  Copy,
  CircleAlert,
  ChevronDown,
  PanelLeftClose,
  PanelLeftOpen,
} from 'lucide-vue-next'

interface Props {
  currentBranch: string
  /** 分支列表是否已加载完成：用于区分「加载中」与「真的游离 HEAD」 */
  branchListReady?: boolean
  repositoryPath: string
  /** 推送反馈状态：done = 推送成功，按钮短暂显示"已推送"并隐藏 Tooltip */
  pushState?: 'idle' | 'done'
  /** 拉取进行中：工具栏按钮统一禁用（busy），进度反馈只留在「变更文件」面板 */
  pulling?: boolean
  /** 推送进行中：同上 */
  pushing?: boolean
  /** 本地+远程分支列表（用于同步状态与分支切换） */
  branches?: Branch[]
  /** 上游跟踪分支名，如 origin/main；无上游为 null */
  upstream?: string | null
  /** 工作区变更计数 */
  workingTree?: { modified: number; untracked: number; conflict: number }
  /** 远程仓库地址 */
  remoteUrl?: string | null
  /** 仓库名（路径末段） */
  repoName?: string
  /** 上次成功拉取的时间戳（客户端记录，非后端） */
  lastPullTime?: number | null
  /** 当前主视图：用于工具栏「切换工作/历史视图」按钮的激活态与提示 */
  currentView?: 'worktree' | 'log'
  /** 左侧侧边栏（仓库/分支/子模块整列）是否已折叠：用于工具栏最左「隐藏/显示侧边栏」按钮的图标与高亮态 */
  sidebarCollapsed?: boolean
}

const props = withDefaults(defineProps<Props>(), {
  branchListReady: false,
  pushState: 'idle',
  pulling: false,
  pushing: false,
  branches: () => [],
  upstream: null,
  workingTree: () => ({ modified: 0, untracked: 0, conflict: 0 }),
  remoteUrl: null,
  repoName: '',
  lastPullTime: null,
  currentView: 'worktree',
  sidebarCollapsed: false,
})
const emit = defineEmits([
  'open-repo', 'commit', 'push', 'pull', 'branch', 'refresh',
  'show-working-tree', 'show-log', 'stash', 'checkout-branch',
  'toggle-sidebar',
])

const hasRepo = computed(() => !!props.repositoryPath)

// 拉取/推送进行中：工具栏操作按钮统一置为不可用，结束后恢复（古哥 2026-10-21）
const busy = computed(() => props.pulling || props.pushing)

const { t } = useI18n()

interface Tool {
  id: string
  labelKey: string
  hintKey: string
  icon: Component
  action: string
  /** 主操作视觉权重：primary 为主色实心按钮 */
  emphasis?: 'primary'
}

/** 左侧：Git 主流程操作（拉取 → 提交 → 推送） */
const primaryTools: Tool[] = [
  { id: 'pull', labelKey: 'toolbar.pull', hintKey: 'toolbar.pullHint', icon: ArrowDownToLine, action: 'pull' },
  { id: 'commit', labelKey: 'toolbar.commit', hintKey: 'toolbar.commitHint', icon: GitCommitVertical, action: 'commit', emphasis: 'primary' },
  { id: 'push', labelKey: 'toolbar.push', hintKey: 'toolbar.pushHint', icon: ArrowUpFromLine, action: 'push' },
]

/** 左侧：紧邻主流程的常用辅助操作（图标 + 悬浮提示） */
const iconTools: Tool[] = [
  { id: 'stash', labelKey: 'toolbar.stash', hintKey: 'toolbar.stashHint', icon: Archive, action: 'stash' },
]

function handleAction(action: string) {
  emit(action as any)
}

// 视图切换按钮：根据当前主视图在「工作视图 ↔ 历史视图」之间 toggle
const viewToggleActive = computed(() => props.currentView === 'log')
const viewToggleLabel = computed(() =>
  viewToggleActive.value ? t('toolbar.worktree') : t('toolbar.log'),
)
const viewToggleHint = computed(() =>
  viewToggleActive.value ? t('toolbar.worktreeHint') : t('toolbar.logHint'),
)
function handleViewToggle() {
  emit(viewToggleActive.value ? 'show-working-tree' : 'show-log')
}

// 工具栏最左「隐藏/显示侧边栏」按钮：图标与提示随折叠态切换（对齐 VS Code / GitKraken）
const sidebarHint = computed(() =>
  props.sidebarCollapsed ? t('sidebar.show') : t('sidebar.hide'),
)
function handleToggleSidebar() {
  emit('toggle-sidebar')
}

// ===== 工作区状态弹窗 =====
const statusOpen = ref(false)
const copied = ref(false)
const statusWrap = ref<HTMLElement | null>(null)

const displayBranch = computed(() => {
  // 分支列表尚未加载、且无乐观分支名时，显示加载占位，避免误报「游离 HEAD」
  if (!props.branchListReady && !props.currentBranch) return t('toolbar.loadingBranch')
  return props.currentBranch || t('toolbar.detachedHead')
})
const currentBranchData = computed(() => props.branches.find(b => b.is_current) || null)
const ahead = computed(() => currentBranchData.value?.ahead ?? 0)
const behind = computed(() => currentBranchData.value?.behind ?? 0)
const hasUpstream = computed(() => !!props.upstream)
const isClean = computed(
  () => props.workingTree.modified + props.workingTree.untracked + props.workingTree.conflict === 0,
)
const localBranches = computed(() => props.branches.filter(b => !b.is_remote))

const lastPullLabel = computed(() => {
  const ts = props.lastPullTime
  if (!ts) return t('toolbar.lastPullNever')
  const diff = Date.now() - ts
  const min = Math.floor(diff / 60000)
  if (min < 1) return t('toolbar.lastPullJustNow')
  if (min < 60) return t('toolbar.lastPullMin', { n: min })
  const hr = Math.floor(min / 60)
  if (hr < 24) return t('toolbar.lastPullHour', { n: hr })
  const day = Math.floor(hr / 24)
  return t('toolbar.lastPullDay', { n: day })
})

function toggleStatus() {
  statusOpen.value = !statusOpen.value
}

function closeStatus() {
  statusOpen.value = false
}

// 拉取/推送开始时自动收起状态弹窗（弹窗内快捷操作已随 busy 禁用，这是双保险）
watch(busy, (b) => {
  if (b) closeStatus()
})

function doAction(action: 'pull' | 'push') {
  emit(action)
  closeStatus()
}

async function copyBranch() {
  const name = props.currentBranch
  if (!name) return
  try {
    await navigator.clipboard.writeText(name)
    copied.value = true
    setTimeout(() => { copied.value = false }, 1500)
  } catch {
    // 剪贴板不可用时静默忽略
  }
}

function switchBranch(name: string) {
  emit('checkout-branch', name)
  closeStatus()
}

// 点击弹窗外部关闭
function onDocMouseDown(e: MouseEvent) {
  const el = statusWrap.value
  if (el && !el.contains(e.target as Node)) closeStatus()
}

// Esc 关闭
function onKeyEscape(e: KeyboardEvent) {
  if (e.key === 'Escape') closeStatus()
}

watch(statusOpen, (open) => {
  if (open) document.addEventListener('mousedown', onDocMouseDown)
  else document.removeEventListener('mousedown', onDocMouseDown)
})

// 切换仓库时收起弹窗
watch(() => props.repositoryPath, () => closeStatus())

onMounted(() => document.addEventListener('keydown', onKeyEscape))
onBeforeUnmount(() => {
  document.removeEventListener('keydown', onKeyEscape)
  document.removeEventListener('mousedown', onDocMouseDown)
})
</script>

<template>
  <TooltipProvider :delay-duration="260">
    <div class="toolbar">
      <!-- 左区：Git 主流程 + 布局控制与常用辅助操作 -->
      <div class="tb-left">
        <!-- Git 主流程：连接式分段按钮（拉取 | 提交 | 推送） -->
        <div class="seg-group">
          <template v-for="tool in primaryTools" :key="tool.id">
            <!-- 推送成功态：不渲染 Tooltip（提示彻底消失），按钮短暂显示"已推送"反馈 -->
            <button
              v-if="tool.id === 'push' && pushState === 'done'"
              type="button"
              class="seg-btn seg-pushed"
              disabled
            >
              <Check :size="15" />
              <span>{{ t('toolbar.pushed') }}</span>
            </button>
            <!-- 常规态：Tooltip 悬浮提示 -->
            <Tooltip v-else>
              <TooltipTrigger as-child>
                <button
                  type="button"
                  class="seg-btn"
                  :class="{ 'is-primary': tool.emphasis === 'primary' }"
                  :disabled="!hasRepo || busy || (tool.action === 'commit' && workingTree.conflict > 0)"
                  @click="handleAction(tool.action)"
                >
                  <component :is="tool.icon" :size="15" />
                  <span>{{ t(tool.labelKey) }}</span>
                </button>
              </TooltipTrigger>
              <TooltipContent side="bottom">{{ t(tool.hintKey) }}</TooltipContent>
            </Tooltip>
          </template>
        </div>

        <!-- 常用辅助操作：隐藏/显示侧边栏 · 储藏 · 工作·历史视图切换 · 刷新 -->
        <div class="tb-quick">
          <!-- 隐藏/显示侧边栏（仓库 / 分支 / 子模块整列）。折叠态高亮，提示可一键展开 -->
          <Tooltip>
            <TooltipTrigger as-child>
              <button
                type="button"
                class="icon-btn"
                :class="{ 'is-active': sidebarCollapsed }"
                :aria-label="sidebarHint"
                :aria-pressed="sidebarCollapsed"
                @click="handleToggleSidebar"
              >
                <PanelLeftOpen v-if="sidebarCollapsed" :size="17" />
                <PanelLeftClose v-else :size="17" />
              </button>
            </TooltipTrigger>
            <TooltipContent side="bottom">{{ sidebarHint }}</TooltipContent>
          </Tooltip>

          <Tooltip v-for="tool in iconTools" :key="tool.id">
            <TooltipTrigger as-child>
              <button
                type="button"
                class="icon-btn"
                :aria-label="t(tool.labelKey)"
                :disabled="!hasRepo || busy"
                @click="handleAction(tool.action)"
              >
                <component :is="tool.icon" :size="16" />
              </button>
            </TooltipTrigger>
            <TooltipContent side="bottom">{{ t(tool.hintKey) }}</TooltipContent>
          </Tooltip>

          <!-- 切换工作视图 / 历史视图（根据当前视图 toggle） -->
          <Tooltip>
            <TooltipTrigger as-child>
              <button
                type="button"
                class="icon-btn"
                :class="{ 'is-active': viewToggleActive }"
                :aria-label="viewToggleLabel"
                :aria-pressed="viewToggleActive"
                :disabled="!hasRepo || busy"
                @click="handleViewToggle"
              >
                <History :size="16" />
              </button>
            </TooltipTrigger>
            <TooltipContent side="bottom">{{ viewToggleHint }}</TooltipContent>
          </Tooltip>

          <Tooltip>
            <TooltipTrigger as-child>
              <button
                type="button"
                class="icon-btn"
                :aria-label="t('toolbar.refresh')"
                :disabled="busy"
                @click="handleAction('refresh')"
              >
                <RefreshCw :size="16" />
              </button>
            </TooltipTrigger>
            <TooltipContent side="bottom">{{ t('toolbar.refreshHint') }}</TooltipContent>
          </Tooltip>
        </div>
      </div>

      <div class="tb-spacer" />

      <!-- 右区：仓库上下文（当前分支 + 同步状态 / 工作区变更 / 视图切换） -->
      <div class="tb-right">
        <div v-if="hasRepo" class="branch-wrap" ref="statusWrap">
          <Tooltip v-if="!statusOpen">
            <TooltipTrigger as-child>
              <button
                type="button"
                class="branch-btn"
                :aria-label="t('toolbar.wsStatus')"
                :aria-expanded="statusOpen"
                @click="toggleStatus"
              >
                <GitBranch :size="14" class="branch-icon" />
                <span class="branch-name">{{ displayBranch }}</span>
                <span v-if="ahead > 0" class="sync-pill ahead">↑{{ ahead }}</span>
                <span v-if="behind > 0" class="sync-pill behind">↓{{ behind }}</span>
                <ChevronDown :size="12" class="branch-chevron" />
              </button>
            </TooltipTrigger>
            <TooltipContent side="bottom">{{ t('toolbar.wsStatus') }}</TooltipContent>
          </Tooltip>
          <button
            v-else
            type="button"
            class="branch-btn is-open"
            :aria-label="t('toolbar.wsStatus')"
            :aria-expanded="statusOpen"
            @click="toggleStatus"
          >
            <GitBranch :size="14" class="branch-icon" />
            <span class="branch-name">{{ displayBranch }}</span>
            <span v-if="ahead > 0" class="sync-pill ahead">↑{{ ahead }}</span>
            <span v-if="behind > 0" class="sync-pill behind">↓{{ behind }}</span>
            <ChevronDown :size="12" class="branch-chevron is-open" />
          </button>

          <!-- 状态弹窗 ① 分支与同步 / ② 工作区 / ③ 仓库身份 / ④ 快捷操作 / ⑤ 切换分支 -->
          <div v-if="statusOpen" class="status-popover">
            <!-- ① 分支与同步 -->
            <div class="sp-section sp-header">
              <div class="sp-pop-title">{{ t('toolbar.wsStatus') }}</div>
              <div class="sp-branch-row">
                <GitBranch :size="14" class="sp-ico" />
                <span class="sp-branch-name">{{ displayBranch }}</span>
                <span v-if="hasUpstream" class="sp-sync">
                  <span v-if="ahead > 0" class="sp-ahead">↑{{ ahead }}</span>
                  <span v-if="behind > 0" class="sp-behind">↓{{ behind }}</span>
                  <span v-if="ahead === 0 && behind === 0" class="sp-synced">{{ t('toolbar.synced') }}</span>
                </span>
              </div>
              <div class="sp-meta">
                <template v-if="hasUpstream">
                  <span class="sp-upstream">↳ {{ upstream }}</span>
                </template>
                <span v-else class="sp-warn">
                  <CircleAlert :size="12" />
                  {{ t('toolbar.noUpstream') }}
                </span>
              </div>
              <div class="sp-meta sp-sub">{{ lastPullLabel }}</div>
            </div>

            <!-- ② 工作区概览（点击查看变更） -->
            <div class="sp-section sp-wt" :title="t('toolbar.viewChanges')" @click="emit('show-working-tree')">
              <template v-if="isClean">
                <Check :size="14" class="sp-ico ok" />
                <span>{{ t('toolbar.wsClean') }}</span>
              </template>
              <template v-else>
                <span class="sp-chip mod">{{ t('toolbar.modified', { n: workingTree.modified }) }}</span>
                <span class="sp-chip untracked">{{ t('toolbar.untracked', { n: workingTree.untracked }) }}</span>
                <span v-if="workingTree.conflict > 0" class="sp-chip conflict">{{ t('toolbar.conflict', { n: workingTree.conflict }) }}</span>
              </template>
              <span class="sp-wt-go">{{ t('toolbar.viewMore') }}</span>
            </div>

            <!-- ③ 仓库身份 -->
            <div class="sp-section sp-repo">
              <div class="sp-repo-name">{{ repoName || '—' }}</div>
              <div class="sp-repo-url" :title="remoteUrl || ''">{{ remoteUrl || t('toolbar.noRemote') }}</div>
            </div>

            <!-- ④ 快捷操作 -->
            <div class="sp-actions">
              <button class="btn-act" :disabled="!hasRepo || busy" @click="doAction('pull')">
                <ArrowDownToLine :size="14" />
                {{ t('toolbar.pull') }}
              </button>
              <button class="btn-act" :disabled="!hasRepo || busy" @click="doAction('push')">
                <ArrowUpFromLine :size="14" />
                {{ t('toolbar.push') }}
              </button>
              <button class="btn-act" :disabled="!hasRepo || !currentBranch" @click="copyBranch">
                <Copy :size="14" />
                {{ copied ? t('toolbar.copied') : t('toolbar.copyBranchName') }}
              </button>
            </div>

            <!-- ⑤ 切换分支 -->
            <div class="sp-section sp-branches">
              <div class="sp-branches-title">{{ t('toolbar.switchBranch') }}</div>
              <div class="sp-branch-list">
                <div
                  v-for="b in localBranches"
                  :key="b.name"
                  class="sp-branch-item"
                  :class="{ active: b.is_current }"
                  @click="switchBranch(b.name)"
                >
                  <Check v-if="b.is_current" :size="13" class="sp-ico ok" />
                  <GitBranch v-else :size="13" class="sp-ico" />
                  <span class="sp-bi-name">{{ b.name }}</span>
                  <span v-if="b.ahead > 0" class="sp-bi-ahead">↑{{ b.ahead }}</span>
                  <span v-if="b.behind > 0" class="sp-bi-behind">↓{{ b.behind }}</span>
                </div>
              </div>
            </div>
          </div>
        </div>

        <span v-else class="no-repo-chip">
          <GitBranch :size="13" />
          {{ t('toolbar.noRepo') }}
        </span>
      </div>
    </div>
  </TooltipProvider>
</template>

<style scoped>
.toolbar {
  display: flex;
  align-items: center;
  gap: 10px;
  height: 44px;
  flex-shrink: 0;
  padding: 0 12px;
  background-color: var(--bg-toolbar);
  border-bottom: 1px solid var(--border-color);
  -webkit-user-select: none;
  -webkit-user-drag: none;
  user-select: none;
}

/* 左右分区：左侧动作、右侧上下文，中间弹性留白 */
.tb-left,
.tb-right {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.tb-left {
  flex-shrink: 0;
}

.tb-right {
  flex-shrink: 0;
}

.tb-spacer {
  flex: 1;
  min-width: 12px;
}

.tb-quick {
  display: flex;
  align-items: center;
  gap: 2px;
}

/* ===== 通用图标按钮 ===== */
.icon-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 30px;
  height: 30px;
  padding: 0;
  border: 0;
  border-radius: 7px;
  background: transparent;
  color: var(--text-tertiary);
  cursor: pointer;
  transition: background-color 0.13s ease, color 0.13s ease;
}

.icon-btn:hover:not(:disabled) {
  background-color: var(--bg-hover);
  color: var(--text-bright);
}

.icon-btn:disabled {
  opacity: 0.42;
  cursor: not-allowed;
}

.icon-btn.is-active {
  background-color: var(--bg-active);
  color: var(--accent-text);
}

.toolbar button:focus-visible {
  outline: 2px solid var(--accent-primary);
  outline-offset: 1px;
}

/* ===== 左侧主流程分段按钮（拉取 | 提交 | 推送）：无边框的凹陷胶囊 ===== */
.seg-group {
  display: flex;
  align-items: center;
  gap: 2px;
  padding: 3px;
  border-radius: 10px;
  background-color: var(--bg-primary);
}

.seg-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  height: 30px;
  padding: 0 12px;
  border: 0;
  border-radius: 7px;
  background: transparent;
  color: var(--text-secondary);
  font-size: 12.5px;
  font-weight: 500;
  white-space: nowrap;
  cursor: pointer;
  transition: background-color 0.13s ease, color 0.13s ease;
}

.seg-btn svg {
  flex-shrink: 0;
}

.seg-btn:hover:not(:disabled) {
  background-color: var(--bg-hover);
  color: var(--text-bright);
}

.seg-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

/* 提交：保持透明底，仅用主色文字/图标区分主操作 */
.seg-btn.is-primary {
  color: var(--brand-primary);
}

.seg-btn.is-primary:hover:not(:disabled) {
  background-color: var(--bg-hover);
  color: var(--brand-primary);
}

/* 推送成功反馈：绿色勾 + "已推送"，不响应点击 */
.seg-pushed,
.seg-pushed:disabled {
  color: var(--color-add);
  opacity: 1;
  cursor: default;
}

/* ===== 右侧：当前分支徽章 ===== */
.branch-wrap {
  position: relative;
  display: inline-flex;
  min-width: 0;
}

.branch-btn {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  height: 32px;
  min-width: 0;
  max-width: 300px;
  padding: 0 9px 0 11px;
  border: 0;
  border-radius: 9px;
  background-color: var(--brand-bg);
  color: var(--text-primary);
  font-size: 12px;
  font-weight: 500;
  cursor: pointer;
  transition: background-color 0.13s ease;
}

.branch-btn:hover:not(:disabled),
.branch-btn.is-open {
  background-color: rgba(59, 130, 246, 0.2);
}

.branch-icon {
  color: var(--accent-text);
  flex-shrink: 0;
}

.branch-name {
  min-width: 0;
  font-family: ui-monospace, 'SF Mono', SFMono-Regular, Menlo, Consolas, monospace;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.branch-chevron {
  margin-left: 1px;
  color: var(--text-muted);
  flex-shrink: 0;
  transition: transform 0.15s ease;
}

.branch-chevron.is-open {
  transform: rotate(180deg);
}

.sync-pill {
  display: inline-flex;
  align-items: center;
  height: 17px;
  padding: 0 5px;
  border-radius: 999px;
  font-size: 10.5px;
  font-weight: 700;
  font-family: ui-monospace, 'SF Mono', SFMono-Regular, Menlo, Consolas, monospace;
  flex-shrink: 0;
}

.sync-pill.ahead {
  background-color: var(--bg-add);
  color: var(--color-add);
}

.sync-pill.behind {
  background-color: var(--bg-mod);
  color: var(--color-mod);
}

.no-repo-chip {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  height: 32px;
  padding: 0 6px;
  font-size: 12px;
  color: var(--text-muted);
}

/* 窄窗口：分段按钮只留图标，把空间让给右侧仓库上下文 */
@media (max-width: 960px) {
  .seg-btn span {
    display: none;
  }

  .seg-btn {
    padding: 0 10px;
  }

  .branch-btn {
    max-width: 200px;
  }
}

/* ===== 工作区状态弹窗 ===== */
.status-popover {
  position: absolute;
  top: calc(100% + 8px);
  right: 0;
  width: 312px;
  max-width: 86vw;
  background-color: var(--bg-secondary);
  border: 1px solid var(--border-medium);
  border-radius: 12px;
  box-shadow: var(--shadow-dialog);
  z-index: 9999;
  padding: 0;
  overflow: hidden;
  text-align: left;
  animation: popover-in 0.12s ease-out;
}

@keyframes popover-in {
  from {
    opacity: 0;
    transform: translateY(-4px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}

.sp-section {
  padding: 10px 14px;
  border-bottom: 1px solid var(--border-color);
}

.sp-header {
  background-color: var(--bg-tertiary);
}

.sp-pop-title {
  font-size: 11px;
  color: var(--text-tertiary);
  font-weight: 600;
  text-transform: uppercase;
  margin-bottom: 6px;
  letter-spacing: 0.04em;
}

.sp-branch-row {
  display: flex;
  align-items: center;
  gap: 8px;
}

.sp-branch-name {
  flex: 1;
  min-width: 0;
  font-family: ui-monospace, 'SF Mono', SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 13px;
  font-weight: 600;
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.sp-ico {
  color: var(--accent-text);
  flex-shrink: 0;
}

.sp-ico.ok {
  color: var(--color-add, #4ec9b0);
}

.sp-sync {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
  font-size: 12px;
  font-family: ui-monospace, 'SF Mono', SFMono-Regular, Menlo, Consolas, monospace;
}

.sp-ahead {
  color: var(--color-add, #4ec9b0);
  font-weight: 600;
}

.sp-behind {
  color: var(--color-mod, #e2c08d);
  font-weight: 600;
}

.sp-synced {
  color: var(--text-muted);
  font-size: 11px;
}

.sp-meta {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-top: 6px;
  font-size: 11px;
  color: var(--text-tertiary);
}

.sp-upstream {
  font-family: ui-monospace, 'SF Mono', SFMono-Regular, Menlo, Consolas, monospace;
}

.sp-warn {
  color: var(--color-mod, #e2c08d);
}

.sp-sub {
  color: var(--text-muted);
}

.sp-wt {
  display: flex;
  align-items: center;
  gap: 8px;
  cursor: pointer;
  font-size: 12px;
  color: var(--text-secondary);
}

.sp-wt:hover {
  background-color: var(--bg-hover);
}

.sp-wt-go {
  margin-left: auto;
  color: var(--text-tertiary);
  font-size: 11px;
  flex-shrink: 0;
}

.sp-chip {
  font-size: 11px;
  padding: 2px 8px;
  border-radius: 999px;
  font-weight: 600;
  font-family: ui-monospace, 'SF Mono', SFMono-Regular, Menlo, Consolas, monospace;
}

.sp-chip.mod {
  background-color: var(--bg-mod, rgba(226, 192, 141, 0.15));
  color: var(--color-mod, #e2c08d);
}

.sp-chip.untracked {
  background-color: var(--bg-add, rgba(78, 201, 176, 0.15));
  color: var(--color-add, #4ec9b0);
}

.sp-chip.conflict {
  background-color: var(--bg-del, rgba(239, 68, 68, 0.15));
  color: var(--color-del, #ef4444);
}

.sp-repo-name {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.sp-repo-url {
  margin-top: 4px;
  font-size: 11px;
  color: var(--text-tertiary);
  font-family: ui-monospace, 'SF Mono', SFMono-Regular, Menlo, Consolas, monospace;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.sp-actions {
  display: flex;
  gap: 6px;
  padding: 10px 14px;
  border-bottom: 1px solid var(--border-color);
}

.btn-act {
  flex: 1;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  height: 30px;
  font-size: 12px;
  border: 1px solid var(--border-medium);
  border-radius: 6px;
  background-color: var(--bg-primary);
  color: var(--text-secondary);
  cursor: pointer;
  transition: background-color 0.13s ease, color 0.13s ease;
}

.btn-act:hover:not(:disabled) {
  background-color: var(--bg-hover);
  color: var(--text-primary);
}

.btn-act:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.sp-branches {
  padding: 8px 0 6px;
  border-bottom: none;
}

.sp-branches-title {
  padding: 0 14px 6px;
  font-size: 11px;
  color: var(--text-tertiary);
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.04em;
}

.sp-branch-list {
  max-height: 180px;
  overflow: auto;
}

.sp-branch-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 14px;
  cursor: pointer;
  font-size: 12px;
  color: var(--text-secondary);
}

.sp-branch-item:hover {
  background-color: var(--bg-hover);
}

.sp-branch-item.active {
  background-color: var(--bg-active);
  color: var(--text-bright);
}

.sp-bi-name {
  flex: 1;
  min-width: 0;
  font-family: ui-monospace, 'SF Mono', SFMono-Regular, Menlo, Consolas, monospace;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.sp-bi-ahead {
  color: var(--color-add, #4ec9b0);
  font-weight: 600;
  font-size: 11px;
  font-family: ui-monospace, 'SF Mono', SFMono-Regular, Menlo, Consolas, monospace;
}

.sp-bi-behind {
  color: var(--color-mod, #e2c08d);
  font-weight: 600;
  font-size: 11px;
  font-family: ui-monospace, 'SF Mono', SFMono-Regular, Menlo, Consolas, monospace;
}
</style>
