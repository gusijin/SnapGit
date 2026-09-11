<script setup lang="ts">
import { ref, computed, watch, onMounted, onBeforeUnmount } from 'vue'
import type { Component } from 'vue'
import { useI18n } from 'vue-i18n'
import type { Branch } from '../types'
import { Button } from '@/components/ui/button'
import { Separator } from '@/components/ui/separator'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip'
import {
  ArrowDownToLine,
  ArrowUpFromLine,
  GitCommitVertical,
  RefreshCw,
  FolderGit2,
  Archive,
  GitBranch,
  Check,
  Loader2,
  Copy,
  CircleAlert,
} from 'lucide-vue-next'

interface Props {
  currentBranch: string
  /** 分支列表是否已加载完成：用于区分「加载中」与「真的游离 HEAD」 */
  branchListReady?: boolean
  repositoryPath: string
  /** 推送反馈状态：done = 推送成功，按钮短暂显示"已推送"并隐藏 Tooltip */
  pushState?: 'idle' | 'done'
  /** 拉取中：按钮显示 spinner + "拉取中…" */
  pulling?: boolean
  /** 推送中：按钮显示 spinner + "推送中…" */
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
})
const emit = defineEmits([
  'open-repo', 'commit', 'push', 'pull', 'branch', 'refresh',
  'show-working-tree', 'stash', 'checkout-branch',
])

const hasRepo = computed(() => !!props.repositoryPath)

const { t } = useI18n()

interface Tool {
  id: string
  labelKey: string
  hintKey: string
  icon: Component
  action: string
  /** 主操作视觉权重：primary 为实心主色 */
  emphasis?: 'primary'
}

/** 左侧：Git 主流程操作（拉取 → 提交 → 推送） */
const primaryTools: Tool[] = [
  { id: 'pull', labelKey: 'toolbar.pull', hintKey: 'toolbar.pullHint', icon: ArrowDownToLine, action: 'pull' },
  { id: 'commit', labelKey: 'toolbar.commit', hintKey: 'toolbar.commitHint', icon: GitCommitVertical, action: 'commit', emphasis: 'primary' },
  { id: 'push', labelKey: 'toolbar.push', hintKey: 'toolbar.pushHint', icon: ArrowUpFromLine, action: 'push' },
]

/** 右侧：辅助操作，仅图标 + 悬浮提示（工作区状态单独渲染为弹窗） */
const iconTools: Tool[] = [
  { id: 'stash', labelKey: 'toolbar.stash', hintKey: 'toolbar.stashHint', icon: Archive, action: 'stash' },
]

function handleAction(action: string) {
  emit(action as any)
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
      <!-- 左：Git 主流程操作组（连接式分段按钮） -->
      <div class="seg-group">
        <template v-for="tool in primaryTools" :key="tool.id">
          <!-- 拉取中态：按钮显示 spinner + "拉取中…" 并禁用 -->
          <Button
            v-if="tool.id === 'pull' && pulling"
            variant="ghost"
            class="seg-btn seg-btn-loading"
            disabled
          >
            <Loader2 :size="18" class="spin-icon" />
            <span>{{ t('toolbar.pulling') }}</span>
          </Button>
          <!-- 推送中态：按钮显示 spinner + "推送中…" 并禁用（优先级高于 done 成功态） -->
          <Button
            v-else-if="tool.id === 'push' && pushing"
            variant="ghost"
            class="seg-btn seg-btn-loading"
            disabled
          >
            <Loader2 :size="18" class="spin-icon" />
            <span>{{ t('toolbar.pushing') }}</span>
          </Button>
          <!-- 推送成功态：不渲染 Tooltip（提示彻底消失），按钮短暂显示"已推送"反馈 -->
          <Button
            v-else-if="tool.id === 'push' && pushState === 'done'"
            variant="ghost"
            class="seg-btn seg-pushed"
            :disabled="!hasRepo"
          >
            <Check :size="18" />
            <span>{{ t('toolbar.pushed') }}</span>
          </Button>
          <!-- 常规态：Tooltip 悬浮提示 -->
          <Tooltip v-else>
            <TooltipTrigger as-child>
              <Button
                variant="ghost"
                :class="tool.emphasis === 'primary' ? 'seg-btn seg-commit' : 'seg-btn'"
                :disabled="!hasRepo"
                @click="handleAction(tool.action)"
              >
                <component :is="tool.icon" />
                <span>{{ t(tool.labelKey) }}</span>
              </Button>
            </TooltipTrigger>
            <TooltipContent side="bottom">{{ t(tool.hintKey) }}</TooltipContent>
          </Tooltip>
        </template>
      </div>

      <!-- 中：当前分支徽章（仅展示，不可点击） -->
      <div class="toolbar-center">
        <Tooltip v-if="hasRepo">
          <TooltipTrigger as-child>
            <span class="branch-chip" role="status">
              <GitBranch :size="13" class="branch-chip-icon" />
              <span class="branch-chip-name">{{ displayBranch }}</span>
            </span>
          </TooltipTrigger>
          <TooltipContent side="bottom">{{ t('toolbar.currentBranch') }}</TooltipContent>
        </Tooltip>
        <span v-else class="no-repo-hint">{{ t('toolbar.noRepo') }}</span>
      </div>

      <!-- 右：辅助操作 -->
      <div class="tool-cluster">
        <!-- 工作区状态：按钮 + 状态弹窗 -->
        <div class="status-wrap" ref="statusWrap">
          <Tooltip v-if="!statusOpen">
            <TooltipTrigger as-child>
              <Button
                variant="ghost"
                size="icon-sm"
                class="h-8 w-8"
                :class="{ 'status-active': statusOpen }"
                :aria-label="t('toolbar.wsStatus')"
                :aria-expanded="statusOpen"
                :disabled="!hasRepo"
                @click="toggleStatus"
              >
                <FolderGit2 />
              </Button>
            </TooltipTrigger>
            <TooltipContent side="bottom">{{ t('toolbar.wsStatus') }}</TooltipContent>
          </Tooltip>
          <Button
            v-else
            variant="ghost"
            size="icon-sm"
            class="h-8 w-8 status-active"
            :aria-label="t('toolbar.wsStatus')"
            :aria-expanded="statusOpen"
            :disabled="!hasRepo"
            @click="toggleStatus"
          >
            <FolderGit2 />
          </Button>

          <!-- 状态弹窗 ① 分支与同步 / ② 工作区 / ③ 仓库身份 / ④ 快捷操作 -->
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
              <button class="btn-act" :disabled="!hasRepo" @click="doAction('pull')">
                <ArrowDownToLine :size="14" />
                {{ t('toolbar.pull') }}
              </button>
              <button class="btn-act" :disabled="!hasRepo" @click="doAction('push')">
                <ArrowUpFromLine :size="14" />
                {{ t('toolbar.push') }}
              </button>
              <button class="btn-act" :disabled="!hasRepo || !currentBranch" @click="copyBranch">
                <Copy :size="14" />
                {{ copied ? t('toolbar.copied') : t('toolbar.copyBranchName') }}
              </button>
            </div>

            <!-- ④ 切换分支 -->
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

        <Tooltip v-for="tool in iconTools" :key="tool.id">
          <TooltipTrigger as-child>
            <Button
              variant="ghost"
              size="icon-sm"
              class="h-8 w-8"
              :aria-label="t(tool.labelKey)"
              :disabled="!hasRepo"
              @click="handleAction(tool.action)"
            >
              <component :is="tool.icon" />
            </Button>
          </TooltipTrigger>
          <TooltipContent side="bottom">{{ t(tool.hintKey) }}</TooltipContent>
        </Tooltip>

        <Separator orientation="vertical" class="mx-1 h-5" />

        <Tooltip>
          <TooltipTrigger as-child>
            <Button
              variant="ghost"
              size="icon-sm"
              class="h-8 w-8"
              :aria-label="t('toolbar.refresh')"
              @click="handleAction('refresh')"
            >
              <RefreshCw />
            </Button>
          </TooltipTrigger>
          <TooltipContent side="bottom">{{ t('toolbar.refreshHint') }}</TooltipContent>
        </Tooltip>
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
  padding: 0 10px;
  background-color: var(--bg-toolbar);
  border-bottom: 1px solid var(--border-color);
  -webkit-user-select: none;
  -webkit-user-drag: none;
  user-select: none;
}

/* 操作分组容器：深色下呈凹陷、浅色下呈白色凸起药丸 */
.tool-cluster {
  display: flex;
  align-items: center;
  gap: 2px;
  padding: 3px;
  border-radius: 9px;
  background-color: var(--bg-primary);
  border: 1px solid var(--border-light);
}

.toolbar-center {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  min-width: 0;
}

.branch-chip {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  height: 28px;
  max-width: 320px;
  padding: 0 11px;
  border-radius: 999px;
  border: 1px solid transparent;
  background-color: var(--brand-bg);
  color: var(--text-primary);
  font-size: 12px;
  font-weight: 500;
  cursor: default;
}

.branch-chip-icon {
  color: var(--accent-text);
  flex-shrink: 0;
}

.branch-chip-name {
  font-family: ui-monospace, 'SF Mono', SFMono-Regular, Menlo, Consolas, monospace;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.no-repo-hint {
  font-size: 12px;
  color: var(--text-muted);
}

/* 左侧 Git 主流程：连接式分段按钮（拉取 | 提交 | 推送）
   三个按钮连成一个胶囊、相邻用分隔线，提交用 tinted 主色柔和强调（非实心亮蓝） */
.seg-group {
  display: flex;
  align-items: stretch;
  padding: 3px;
  border-radius: 9px;
  background-color: var(--bg-primary);
  border: 1px solid var(--border-light);
  overflow: hidden;
}

.seg-group :deep(.seg-btn) {
  position: relative;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  height: 30px;
  padding: 0 11px;
  border: none;
  border-radius: 0;
  box-shadow: none;
  font-size: 12px;
  font-weight: 500;
  background: transparent;
  color: var(--text-secondary);
  transition: background-color 0.13s ease, color 0.13s ease;
}

.seg-group :deep(.seg-btn:hover):not(:disabled) {
  background: var(--bg-hover);
  color: var(--text-primary);
}

.seg-group :deep(.seg-btn:disabled) {
  opacity: 0.4;
  cursor: not-allowed;
}

/* 相邻按钮间 1px 分隔线（首个除外） */
.seg-group :deep(.seg-btn + .seg-btn)::before {
  content: '';
  position: absolute;
  left: 0;
  top: 50%;
  transform: translateY(-50%);
  height: 16px;
  width: 1px;
  background: var(--border-medium);
}

/* 提交：背景与拉取一致（透明），仅文字/图标保留主色以区分主操作 */
.seg-group :deep(.seg-commit) {
  color: var(--brand-primary);
}

.seg-group :deep(.seg-commit:hover):not(:disabled) {
  background: var(--bg-hover);
  color: var(--brand-primary);
}

/* 推送成功反馈：主题主色"已推送" */
.seg-group :deep(.seg-pushed) {
  color: var(--brand-primary);
  cursor: default;
}

.seg-group :deep(.seg-pushed:hover):not(:disabled) {
  background: transparent;
  color: var(--brand-primary);
}

/* 拉取/推送中反馈：禁用按钮 + 图标旋转动画 */
.seg-group :deep(.seg-btn-loading) {
  color: var(--text-secondary);
  cursor: progress;
  opacity: 0.85;
}

.spin-icon {
  animation: toolbar-spin 0.9s linear infinite;
}

@keyframes toolbar-spin {
  to { transform: rotate(360deg); }
}

/* ===== 工作区状态弹窗 ===== */
.status-wrap {
  position: relative;
  display: inline-flex;
}

.status-wrap :deep(.status-active) {
  background-color: var(--bg-active);
  color: var(--text-bright);
}

.status-popover {
  position: absolute;
  top: calc(100% + 8px);
  right: 0;
  width: 300px;
  max-width: 86vw;
  background-color: var(--bg-secondary);
  border: 1px solid var(--border-medium);
  border-radius: 10px;
  box-shadow: var(--shadow-dialog);
  z-index: 9999;
  padding: 0;
  overflow: hidden;
  text-align: left;
  animation: fadeIn 0.1s ease-out;
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
