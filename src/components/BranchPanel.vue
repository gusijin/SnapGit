<script setup lang="ts">
import { ref, reactive, computed, watch, onMounted, onBeforeUnmount } from 'vue'
import { useI18n } from 'vue-i18n'
import type { Branch, StashEntry } from '../types'
import { getUpstream, checkRemoteBranchDeletable } from '../api/git'
import { GitBranchPlus, ArrowUpFromLine, ArrowDownToLine, GitMerge, Pencil, Download, Archive, Sparkles, Trash2, ChevronDown, ChevronRight } from 'lucide-vue-next'

const { t } = useI18n()

interface Props {
  repoPath: string
  branches: Branch[]
  currentBranch: string
  checkoutError: string | null
  checkoutTargetBranch: string
  stashes: StashEntry[]
}

const props = defineProps<Props>()
const emit = defineEmits([
  'checkout-branch', 'checkout-fast-forward', 'checkout-force', 'checkout-cancel',
  'create-branch', 'merge-branch', 'rename-branch', 'push-branch', 'pull-branch',
  'checkout-remote', 'stash-apply', 'stash-drop', 'delete-branch', 'delete-remote-branch',
])

const checkoutErrorDialogVisible = ref(false)

watch(() => props.checkoutError, (val) => {
  if (val) {
    checkoutErrorDialogVisible.value = true
  }
})

function checkout(name: string) {
  emit('checkout-branch', name)
}

function confirmFastForward() {
  checkoutErrorDialogVisible.value = false
  emit('checkout-fast-forward', props.checkoutTargetBranch)
}

function confirmForceOnly() {
  checkoutErrorDialogVisible.value = false
  emit('checkout-force', props.checkoutTargetBranch)
}

function cancelCheckout() {
  checkoutErrorDialogVisible.value = false
  emit('checkout-cancel')
}

const localBranches = computed(() => props.branches.filter(b => !b.is_remote))
const remoteBranches = computed(() => props.branches.filter(b => b.is_remote))

// ===== 分组折叠 / 展开 =====
const expanded = reactive<{ local: boolean; remote: boolean; stash: boolean }>({
  local: true,
  remote: true,
  stash: true,
})

function toggleGroup(key: 'local' | 'remote' | 'stash') {
  expanded[key] = !expanded[key]
}

// ===== 分支右键菜单 =====
const contextMenu = ref<{ visible: boolean; x: number; y: number; branch: Branch | null }>({
  visible: false,
  x: 0,
  y: 0,
  branch: null,
})

function openContextMenu(e: MouseEvent, branch: Branch) {
  e.preventDefault()
  e.stopPropagation()
  closeStashContextMenu()
  contextMenu.value = { visible: true, x: e.clientX, y: e.clientY, branch }
}

function closeContextMenu() {
  contextMenu.value.visible = false
  contextMenu.value.branch = null
}

// ===== 储藏右键菜单 =====
const stashContextMenu = ref<{ visible: boolean; x: number; y: number; stash: StashEntry | null }>({
  visible: false,
  x: 0,
  y: 0,
  stash: null,
})

function openStashContextMenu(e: MouseEvent, stash: StashEntry) {
  e.preventDefault()
  e.stopPropagation()
  closeContextMenu()
  stashContextMenu.value = { visible: true, x: e.clientX, y: e.clientY, stash }
}

function closeStashContextMenu() {
  stashContextMenu.value.visible = false
  stashContextMenu.value.stash = null
}

function handleStashApplyFromMenu() {
  const stash = stashContextMenu.value.stash
  if (!stash) return
  emit('stash-apply', stash.stash_ref, false)
  closeStashContextMenu()
}

function handleStashDropFromMenu() {
  const stash = stashContextMenu.value.stash
  if (!stash) return
  emit('stash-drop', stash.stash_ref)
  closeStashContextMenu()
}

// 滚动时收起右键菜单（菜单 fixed 定位不会跟随滚动）
function onDocScroll() {
  closeContextMenu()
  closeStashContextMenu()
}

onMounted(() => {
  document.addEventListener('scroll', onDocScroll, true)
})
onBeforeUnmount(() => {
  document.removeEventListener('scroll', onDocScroll, true)
})

// 创建分支对话框
const showCreateDialog = ref(false)
const newBranchName = ref('')

function openCreateDialog() {
  newBranchName.value = ''
  showCreateDialog.value = true
  closeContextMenu()
}

function confirmCreate() {
  const name = newBranchName.value.trim()
  if (!name) return
  emit('create-branch', name)
  showCreateDialog.value = false
  newBranchName.value = ''
}

// 重命名分支对话框
const showRenameDialog = ref(false)
const renameTarget = ref('')
const renameBranchName = ref('')

function openRenameDialog() {
  const branch = contextMenu.value.branch
  if (!branch) return
  renameTarget.value = branch.name
  renameBranchName.value = branch.name
  showRenameDialog.value = true
  closeContextMenu()
}

function confirmRename() {
  const newName = renameBranchName.value.trim()
  if (!renameTarget.value || !newName || newName === renameTarget.value) {
    showRenameDialog.value = false
    return
  }
  emit('rename-branch', { oldName: renameTarget.value, newName })
  showRenameDialog.value = false
  renameTarget.value = ''
  renameBranchName.value = ''
}

// 删除分支对话框
const showDeleteDialog = ref(false)
const deleteTarget = ref('')
const deleteIsRemote = ref(false)
const deleteUpstream = ref<{ remote: string; remote_branch: string } | null>(null)
const deleteRemoteInfo = ref<{ remote: string; remote_branch: string } | null>(null)
const deleteTracking = ref(false)
const deleteRemote = ref(false)
const deleting = ref(false)
// 远程分支是否可被删除（dry-run + 协议检测）。null=检测中/未检测；false=不可删。
const deleteRemoteAvailable = ref<boolean | null>(null)
const deleteRemoteReason = ref<string | null>(null)

async function openDeleteDialog() {
  const branch = contextMenu.value.branch
  if (!branch) return
  deleteTarget.value = branch.name
  deleteTracking.value = false
  deleteRemote.value = false
  deleteUpstream.value = null
  deleteRemoteInfo.value = null
  deleteRemoteAvailable.value = null
  deleteRemoteReason.value = null
  deleting.value = false
  showDeleteDialog.value = true
  closeContextMenu()

  if (branch.is_remote) {
    // 远程分支（如 origin/gsj_new）：拆分出 remote / remote_branch，主操作为从远程删除
    deleteIsRemote.value = true
    if (branch.name.includes('/')) {
      const idx = branch.name.indexOf('/')
      deleteRemoteInfo.value = {
        remote: branch.name.slice(0, idx),
        remote_branch: branch.name.slice(idx + 1),
      }
      if (deleteRemoteInfo.value) {
        await checkRemoteDeleteAvailability(
          deleteRemoteInfo.value.remote,
          deleteRemoteInfo.value.remote_branch,
        )
      }
    }
  } else {
    // 本地分支：读取上游，决定两个复选框是否可勾选
    deleteIsRemote.value = false
    try {
      const upstream = await getUpstream(props.repoPath, branch.name)
      deleteUpstream.value = upstream
      if (upstream) {
        await checkRemoteDeleteAvailability(upstream.remote, upstream.remote_branch)
      } else {
        deleteRemoteAvailable.value = null
        deleteRemoteReason.value = null
      }
    } catch {
      deleteUpstream.value = null
      deleteRemoteAvailable.value = null
      deleteRemoteReason.value = null
    }
  }
}

/**
 * 检测远程分支可删除性。结果写入 deleteRemoteAvailable / deleteRemoteReason。
 * 检测中时（available===null）复选框保持默认态，不显示灰；用户首次勾选后才会被禁用。
 * 这里"检测中→默认可用、检测后→按结果启用/禁用"。
 */
async function checkRemoteDeleteAvailability(remote: string, remoteBranch: string) {
  // 先标记为检测中，让 UI 给个轻量提示（保留勾选状态，避免闪烁）
  deleteRemoteAvailable.value = null
  deleteRemoteReason.value = null
  try {
    const result = await checkRemoteBranchDeletable(props.repoPath, remote, remoteBranch)
    deleteRemoteAvailable.value = result.can_delete
    deleteRemoteReason.value = result.reason
    // 若不可删，确保复选框处于未勾选态（避免之前的勾选残留）
    if (!result.can_delete) {
      deleteRemote.value = false
    }
  } catch {
    // 异常时不阻塞，保持默认可用（与 Rust 端 dry-run 异常处理一致）
    deleteRemoteAvailable.value = true
    deleteRemoteReason.value = null
  }
}

async function confirmDelete() {
  const isRemote = deleteIsRemote.value
  const tracking = deleteTracking.value
  if (deleting.value) return
  deleting.value = true
  showDeleteDialog.value = false

  if (isRemote && deleteRemoteInfo.value) {
    emit('delete-remote-branch', {
      remote: deleteRemoteInfo.value.remote,
      remoteBranch: deleteRemoteInfo.value.remote_branch,
      deleteTracking: tracking,
    })
  } else {
    emit('delete-branch', {
      name: deleteTarget.value,
      deleteTracking: tracking,
      deleteRemote: deleteRemote.value,
    })
  }
}

// 合并 / 推送
function handleMerge() {
  const branch = contextMenu.value.branch
  if (branch) emit('merge-branch', branch.name)
  closeContextMenu()
}

function handlePush() {
  const branch = contextMenu.value.branch
  if (branch) emit('push-branch', branch.name)
  closeContextMenu()
}

// 拉取：仅对当前（已检出）分支有效，复用父组件的 pull 逻辑
function handlePull() {
  emit('pull-branch')
  closeContextMenu()
}

// 检出远程分支：在本地基于该远程分支创建跟踪分支并切换
function handleCheckoutRemote() {
  const branch = contextMenu.value.branch
  if (branch) emit('checkout-remote', branch.name)
  closeContextMenu()
}

// 暴露给父组件（菜单栏等）调用
defineExpose({ openCreateDialog })
</script>

<template>
  <div class="branch-panel">
    <div class="panel-header">
      <span>{{ t('branchPanel.title') }}</span>
    </div>
    <div class="panel-content" @contextmenu.prevent.stop="void 0">
      <div v-if="localBranches.length > 0" class="group">
        <div class="group-title group-branch" @click="toggleGroup('local')">
          <span class="group-title-left">
            <ChevronRight v-if="!expanded.local" :size="12" class="group-caret" />
            <ChevronDown v-else :size="12" class="group-caret" />
            <span>{{ t('branchPanel.local') }}</span>
          </span>
          <span class="group-count">{{ localBranches.length }}</span>
        </div>
        <div v-show="expanded.local" class="group-body">
          <div
            v-for="branch in localBranches"
            :key="branch.name"
            class="branch-item"
            :class="{ active: branch.is_current }"
            @dblclick="!branch.is_current && checkout(branch.name)"
            @contextmenu="openContextMenu($event, branch)"
          >
            <span class="branch-name">{{ branch.name }}</span>
            <span v-if="branch.ahead > 0" class="ahead-badge" :title="t('branchPanel.aheadTitle', { n: branch.ahead })">
              ↑{{ branch.ahead }}
            </span>
            <span v-if="branch.behind > 0" class="behind-badge" :title="t('branchPanel.behindTitle', { n: branch.behind })">
              ↓{{ branch.behind }}
            </span>
            <button
              v-if="!branch.is_current"
              class="checkout-btn"
              :title="t('branchPanel.checkoutTitle')"
              @click.stop="checkout(branch.name)"
            >{{ t('branchPanel.checkout') }}</button>
          </div>
        </div>
      </div>

      <div v-if="remoteBranches.length > 0" class="group">
        <div class="group-title group-branch" @click="toggleGroup('remote')">
          <span class="group-title-left">
            <ChevronRight v-if="!expanded.remote" :size="12" class="group-caret" />
            <ChevronDown v-else :size="12" class="group-caret" />
            <span>{{ t('branchPanel.remote') }}</span>
          </span>
          <span class="group-count">{{ remoteBranches.length }}</span>
        </div>
        <div v-show="expanded.remote" class="group-body">
          <div
            v-for="branch in remoteBranches"
            :key="branch.name"
            class="branch-item remote"
            @contextmenu="openContextMenu($event, branch)"
          >
            <span class="branch-name">{{ branch.name }}</span>
          </div>
        </div>
      </div>

      <!-- 储藏列表 -->
      <div v-if="stashes.length > 0" class="group">
        <div class="group-title" @click="toggleGroup('stash')">
          <span class="group-title-left">
            <ChevronRight v-if="!expanded.stash" :size="12" class="group-caret" />
            <ChevronDown v-else :size="12" class="group-caret" />
            <span>{{ t('branchPanel.stash') }}</span>
          </span>
          <span class="group-count">{{ stashes.length }}</span>
        </div>
        <div v-show="expanded.stash" class="group-body">
          <div
            v-for="stash in stashes"
            :key="stash.stash_ref"
            class="branch-item stash-item"
            :title="t('branchPanel.stashTitle', { message: stash.message || t('branchPanel.stashNoDesc'), branch: stash.branch })"
            @contextmenu="openStashContextMenu($event, stash)"
          >
            <Archive :size="13" class="stash-icon" />
            <span class="stash-label">{{ stash.index }}: {{ stash.message || t('branchPanel.stashNoDesc') }}</span>
            <span class="stash-branch" :title="t('branchPanel.stashBranch')">{{ stash.branch }}</span>
            <div class="stash-actions">
              <button
                class="stash-btn"
                :title="t('branchPanel.applyStashKeep')"
                @click.stop="emit('stash-apply', stash.stash_ref, false)"
              >
                <Sparkles :size="13" />
              </button>
              <button
                class="stash-btn stash-btn-danger"
                :title="t('branchPanel.dropStash')"
                @click.stop="emit('stash-drop', stash.stash_ref)"
              >
                <Trash2 :size="13" />
              </button>
            </div>
          </div>
        </div>
      </div>

      <div v-if="branches.length === 0 && stashes.length === 0" class="empty">
        <span>{{ t('branchPanel.noBranches') }}</span>
      </div>
    </div>

    <!-- 分支右键菜单 -->
    <Teleport to="body">
      <div
        v-if="contextMenu.visible && contextMenu.branch"
        class="context-menu"
        :style="{ left: contextMenu.x + 'px', top: contextMenu.y + 'px' }"
        @click.stop
      >
        <!-- 远程分支：检出（本地创建跟踪分支） -->
        <template v-if="contextMenu.branch.is_remote">
          <div
            class="menu-item"
            :title="t('branchPanel.createLocalTitle', { branch: contextMenu.branch.name })"
            @click="handleCheckoutRemote"
          >
            <Download :size="14" class="menu-icon" />
            <span>{{ t('branchPanel.checkout') }}</span>
          </div>
          <div class="menu-separator"></div>
          <div
            class="menu-item menu-item-danger"
            :title="t('branchPanel.deleteRemoteTitle', { branch: contextMenu.branch.name })"
            @click="openDeleteDialog"
          >
            <Trash2 :size="14" class="menu-icon" />
            <span>{{ t('branchPanel.deleteRemoteBranch') }}</span>
          </div>
        </template>
        <template v-else-if="contextMenu.branch.is_current">
          <div class="menu-item" @click="openCreateDialog">
            <GitBranchPlus :size="14" class="menu-icon" />
            <span>{{ t('branchPanel.createBranch') }}</span>
          </div>
          <div class="menu-item" @click="handlePull">
            <ArrowDownToLine :size="14" class="menu-icon" />
            <span>{{ t('branchPanel.pull') }}</span>
          </div>
          <div class="menu-item" @click="handlePush">
            <ArrowUpFromLine :size="14" class="menu-icon" />
            <span>{{ t('branchPanel.push') }}</span>
          </div>
        </template>
        <template v-else>
          <div
            class="menu-item"
            :title="t('branchPanel.mergeTitle', { branch: contextMenu.branch.name })"
            @click="handleMerge"
          >
            <GitMerge :size="14" class="menu-icon" />
            <span>{{ t('branchPanel.merge') }}</span>
          </div>
          <div class="menu-item" @click="handlePush">
            <ArrowUpFromLine :size="14" class="menu-icon" />
            <span>{{ t('branchPanel.push') }}</span>
          </div>
          <div class="menu-item" @click="openRenameDialog">
            <Pencil :size="14" class="menu-icon" />
            <span>{{ t('branchPanel.rename') }}</span>
          </div>
          <div class="menu-separator"></div>
          <div
            class="menu-item menu-item-danger"
            :title="t('branchPanel.deleteTitle', { branch: contextMenu.branch.name })"
            @click="openDeleteDialog"
          >
            <Trash2 :size="14" class="menu-icon" />
            <span>{{ t('branchPanel.delete') }}</span>
          </div>
        </template>
      </div>

      <div
        v-if="contextMenu.visible"
        class="context-mask"
        @click="closeContextMenu"
        @contextmenu.prevent="closeContextMenu"
      ></div>
    </Teleport>

    <!-- 储藏右键菜单 -->
    <Teleport to="body">
      <div
        v-if="stashContextMenu.visible && stashContextMenu.stash"
        class="context-menu"
        :style="{ left: stashContextMenu.x + 'px', top: stashContextMenu.y + 'px' }"
        @click.stop
      >
        <div class="menu-item" @click="handleStashApplyFromMenu">
          <Sparkles :size="14" class="menu-icon" />
          <span>{{ t('branchPanel.applyStash') }}</span>
        </div>
        <div class="menu-item menu-item-danger" @click="handleStashDropFromMenu">
          <Trash2 :size="14" class="menu-icon" />
          <span>{{ t('branchPanel.dropStash') }}</span>
        </div>
      </div>

      <div
        v-if="stashContextMenu.visible"
        class="context-mask"
        @click="closeStashContextMenu"
        @contextmenu.prevent="closeStashContextMenu"
      ></div>
    </Teleport>

    <!-- 创建分支对话框 -->
    <Teleport to="body">
      <div v-if="showCreateDialog" class="dialog-mask" @click="showCreateDialog = false">
        <div class="dialog" @click.stop>
          <div class="dialog-header">{{ t('branchPanel.createDialogTitle') }}</div>
          <div class="dialog-body dialog-input-body">
            <span class="dialog-icon"><GitBranchPlus :size="22" /></span>
            <div class="dialog-message">
              <p>{{ t('branchPanel.createDialogHint') }}</p>
              <input
                v-model="newBranchName"
                class="input-field"
                :placeholder="t('branchPanel.createPlaceholder')"
                autofocus
                @keydown.enter="confirmCreate"
                @keydown.esc="showCreateDialog = false"
              />
            </div>
          </div>
          <div class="dialog-footer">
            <button class="btn btn-cancel" @click="showCreateDialog = false">{{ t('branchPanel.cancel') }}</button>
            <button class="btn btn-primary" :disabled="!newBranchName.trim()" @click="confirmCreate">{{ t('branchPanel.createBtn') }}</button>
          </div>
        </div>
      </div>
    </Teleport>

    <!-- 重命名分支对话框 -->
    <Teleport to="body">
      <div v-if="showRenameDialog" class="dialog-mask" @click="showRenameDialog = false">
        <div class="dialog" @click.stop>
          <div class="dialog-header">{{ t('branchPanel.renameDialogTitle') }}</div>
          <div class="dialog-body dialog-input-body">
            <span class="dialog-icon"><Pencil :size="22" /></span>
            <div class="dialog-message">
              <p>{{ t('branchPanel.renameDialogHint', { name: renameTarget }) }}</p>
              <input
                v-model="renameBranchName"
                class="input-field"
                :placeholder="t('branchPanel.renamePlaceholder')"
                autofocus
                @keydown.enter="confirmRename"
                @keydown.esc="showRenameDialog = false"
              />
            </div>
          </div>
          <div class="dialog-footer">
            <button class="btn btn-cancel" @click="showRenameDialog = false">{{ t('branchPanel.cancel') }}</button>
            <button
              class="btn btn-primary"
              :disabled="!renameBranchName.trim() || renameBranchName.trim() === renameTarget"
              @click="confirmRename"
            >{{ t('branchPanel.renameBtn') }}</button>
          </div>
        </div>
      </div>
    </Teleport>

    <!-- 删除分支对话框 -->
    <Teleport to="body">
      <div v-if="showDeleteDialog" class="dialog-mask" @click="showDeleteDialog = false">
        <div class="dialog" @click.stop>
          <div class="dialog-header">{{ t('branchPanel.deleteDialogTitle') }}</div>
          <div class="dialog-body">
            <span class="dialog-icon dialog-icon-danger"><Trash2 :size="22" /></span>
            <div class="dialog-message">
              <!-- 本地分支：删除本地分支 + 可选删跟踪/远程 -->
              <template v-if="!deleteIsRemote">
                <p>{{ t('branchPanel.deleteDialogDesc', { branch: deleteTarget }) }}</p>
                <p class="dialog-hint">{{ t('branchPanel.deleteDialogWarning') }}</p>

                <div v-if="deleteUpstream" class="delete-options">
                  <label class="delete-option">
                    <input type="checkbox" v-model="deleteTracking" class="delete-checkbox" />
                    <span class="delete-option-label">
                      {{ t('branchPanel.deleteTracking', { remote: deleteUpstream.remote, remoteBranch: deleteUpstream.remote_branch }) }}
                    </span>
                  </label>
                  <label
                    class="delete-option"
                    :class="{
                      'delete-option-disabled': deleteRemoteAvailable === false,
                      'delete-option-checking': deleteRemoteAvailable === null,
                    }"
                    :title="deleteRemoteAvailable === false ? (deleteRemoteReason || '') : ''"
                  >
                    <input
                      type="checkbox"
                      v-model="deleteRemote"
                      class="delete-checkbox"
                      :disabled="deleteRemoteAvailable !== true"
                    />
                    <span class="delete-option-label">
                      {{ t('branchPanel.deleteRemote', { remote: deleteUpstream.remote, remoteBranch: deleteUpstream.remote_branch }) }}
                      <span v-if="deleteRemoteAvailable === null" class="delete-option-tag">
                        {{ t('branchPanel.deleteRemoteChecking') }}
                      </span>
                      <span v-else-if="deleteRemoteAvailable === false" class="delete-option-tag">
                        {{ t('branchPanel.deleteRemoteUnavailable') }}
                      </span>
                    </span>
                  </label>
                </div>
                <p v-else class="dialog-hint dialog-hint-sub">
                  {{ t('branchPanel.deleteNoUpstream') }}
                </p>
              </template>

              <!-- 远程分支：从远程删除 + 可选删本地跟踪引用 -->
              <template v-else>
                <p>{{ t('branchPanel.deleteRemoteDialogDesc', { remote: deleteRemoteInfo?.remote, remoteBranch: deleteRemoteInfo?.remote_branch }) }}</p>
                <p class="dialog-hint">{{ t('branchPanel.deleteRemoteDialogWarning') }}</p>

                <p
                  v-if="deleteRemoteAvailable === false"
                  class="dialog-hint dialog-hint-sub"
                  style="color: var(--destructive, #ef4444)"
                >
                  {{ deleteRemoteReason || t('branchPanel.deleteRemoteUnavailable') }}
                </p>

                <div v-if="deleteRemoteInfo" class="delete-options">
                  <label class="delete-option">
                    <input type="checkbox" v-model="deleteTracking" class="delete-checkbox" />
                    <span class="delete-option-label">
                      {{ t('branchPanel.deleteTracking', { remote: deleteRemoteInfo.remote, remoteBranch: deleteRemoteInfo.remote_branch }) }}
                    </span>
                  </label>
                </div>
              </template>
            </div>
          </div>
          <div class="dialog-footer">
            <button class="btn btn-cancel" @click="showDeleteDialog = false">{{ t('branchPanel.cancel') }}</button>
            <button
              class="btn btn-danger"
              :disabled="deleting || (deleteIsRemote && deleteRemoteAvailable !== true)"
              @click="confirmDelete"
            >
              {{ deleting ? t('branchPanel.deleting') : t('branchPanel.deleteBtn') }}
            </button>
          </div>
        </div>
      </div>
    </Teleport>

    <!-- Checkout error dialog -->
    <Teleport to="body">
      <div v-if="checkoutErrorDialogVisible" class="dialog-mask" @click="cancelCheckout">
        <div class="dialog" @click.stop>
          <div class="dialog-header">{{ t('branchPanel.checkoutDialogTitle') }}</div>
          <div class="dialog-body">
            <span class="dialog-icon">🔄</span>
            <div class="dialog-message">
              <p>{{ t('branchPanel.checkoutDialogDesc', { branch: checkoutTargetBranch }) }}</p>
              <p class="dialog-hint">{{ t('branchPanel.checkoutDialogHint') }}</p>
            </div>
          </div>
          <div class="dialog-footer">
            <button class="btn btn-secondary" @click="confirmFastForward">{{ t('branchPanel.fastForwardMerge') }}</button>
            <button class="btn btn-primary" @click="confirmForceOnly">{{ t('branchPanel.checkoutOnly') }}</button>
            <button class="btn btn-cancel" @click="cancelCheckout">{{ t('branchPanel.cancel') }}</button>
          </div>
        </div>
      </div>
    </Teleport>
  </div>
</template>

<style scoped>
.branch-panel {
  display: flex;
  flex-direction: column;
  background-color: var(--bg-secondary);
  overflow: hidden;
  -webkit-user-select: none;
  -webkit-user-drag: none;
  user-select: none;
}

.panel-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
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

.panel-content {
  flex: 1;
  overflow: auto;
  padding: 0;
  min-height: 0;
}

.group {
  margin-bottom: 2px;
}

.group:not(:first-child) {
  border-top: 1px solid var(--border-light);
}

.group-title {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 5px 10px 5px 12px;
  font-size: 10px;
  color: var(--text-secondary);
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  background-color: var(--bg-secondary);
  cursor: pointer;
  user-select: none;
}

.group-title:hover {
  background-color: var(--bg-tertiary);
}

.group-title-left {
  display: flex;
  align-items: center;
  gap: 4px;
}

.group-caret {
  flex-shrink: 0;
  color: var(--text-tertiary);
}

.group-count {
  font-size: 10px;
  min-width: 18px;
  height: 16px;
  padding: 0 5px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background-color: var(--bg-tertiary);
  border-radius: 999px;
  color: var(--text-secondary);
  font-weight: 700;
}

/* 本地/远程分支分组的数量 badge 不加粗（储藏栏保持加粗） */
.group-branch .group-count {
  font-weight: 400;
  min-width: auto;
  height: auto;
  padding: 1px 6px;
  display: inline;
  border-radius: 8px;
  color: var(--text-tertiary);
}

.branch-item {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 5px 10px 5px 22px;
  cursor: pointer;
  font-size: 12px;
  color: var(--text-secondary);
  border-radius: 0 5px 5px 0;
  margin-right: 6px;
  transition: background-color 0.12s, color 0.12s;
}

.branch-item:hover {
  background-color: var(--bg-hover);
  color: var(--text-primary);
}

.branch-item.active {
  background-color: var(--bg-selected);
  font-weight: 600;
}

.branch-item.remote {
  color: var(--text-muted);
}

.branch-item.remote:hover {
  color: var(--text-secondary);
}

.branch-name {
  flex: 1;
  font-family: Consolas, Monaco, monospace;
}

.ahead-badge {
  font-size: 10px;
  padding: 1px 5px;
  background-color: var(--bg-add);
  color: var(--color-add);
  border-radius: 8px;
  font-family: Consolas, Monaco, monospace;
  font-weight: 600;
  flex-shrink: 0;
}

.behind-badge {
  font-size: 10px;
  padding: 1px 5px;
  background-color: var(--bg-mod);
  color: var(--color-mod);
  border-radius: 8px;
  font-family: Consolas, Monaco, monospace;
  font-weight: 600;
  flex-shrink: 0;
}

.checkout-btn {
  padding: 2px 8px;
  font-size: 10px;
  background-color: var(--bg-secondary);
  border: 1px solid var(--border-medium);
  color: var(--accent-text);
  border-radius: 4px;
  cursor: pointer;
  opacity: 0;
  transition: opacity 0.15s, background-color 0.12s;
}

.branch-item:hover .checkout-btn {
  opacity: 1;
}

.checkout-btn:hover {
  background-color: var(--bg-tertiary);
  border-color: var(--accent-primary);
  color: var(--text-bright);
}

.empty {
  padding: 16px;
  text-align: center;
  color: var(--text-muted);
  font-size: 12px;
}

/* 右键菜单（与 FileList 一致） */
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

.context-mask {
  position: fixed;
  inset: 0;
  z-index: 9998;
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

.menu-icon {
  flex-shrink: 0;
  color: var(--text-tertiary);
}

.menu-item-danger {
  color: var(--destructive, #ef4444);
}

.menu-item-danger:hover {
  color: var(--destructive, #ef4444);
}

.menu-item-danger .menu-icon {
  color: var(--destructive, #ef4444);
}

.menu-separator {
  height: 1px;
  margin: 4px 0;
  background-color: var(--border-color);
}

/* 输入对话框 */
.dialog-input-body {
  align-items: center;
}

.input-field {
  width: 100%;
  padding: 6px 10px;
  margin-top: 4px;
  font-size: 12px;
  font-family: Consolas, Monaco, monospace;
  color: var(--text-primary);
  background-color: var(--bg-primary);
  border: 1px solid var(--border-medium);
  border-radius: 4px;
  outline: none;
  transition: border-color 0.15s;
}

.input-field:focus {
  border-color: var(--accent-primary);
}

/* Dialog styles */
.dialog-mask {
  position: fixed;
  inset: 0;
  background-color: rgba(0, 0, 0, 0.5);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 9999;
  animation: fadeIn 0.15s ease-out;
}

.dialog {
  background-color: var(--bg-secondary);
  border: 1px solid var(--border-color);
  border-radius: 8px;
  min-width: 360px;
  box-shadow: 0 8px 32px rgba(0, 0, 0, 0.3);
  animation: dialogIn 0.15s ease-out;
}

.dialog-header {
  padding: 12px 16px;
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary);
  border-bottom: 1px solid var(--border-color);
}

.dialog-body {
  display: flex;
  gap: 12px;
  padding: 16px;
  align-items: flex-start;
}

.dialog-icon {
  font-size: 28px;
  flex-shrink: 0;
  line-height: 1;
  color: var(--accent-text);
}

.dialog-message p {
  margin: 0 0 8px 0;
  font-size: 13px;
  color: var(--text-primary);
  line-height: 1.5;
}

.dialog-message p:last-child {
  margin-bottom: 0;
}

.dialog-hint {
  font-size: 12px !important;
  color: var(--text-tertiary) !important;
}

.dialog-footer {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  padding: 12px 16px;
  border-top: 1px solid var(--border-color);
}

.btn {
  padding: 6px 16px;
  font-size: 12px;
  border-radius: 4px;
  cursor: pointer;
  border: 1px solid transparent;
  transition: all 0.15s;
}

.btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.btn-primary {
  background-color: var(--accent-primary);
  color: white;
}

.btn-primary:hover:not(:disabled) {
  opacity: 0.9;
}

.btn-secondary {
  background-color: var(--bg-tertiary);
  color: var(--text-primary);
  border-color: var(--border-color);
}

.btn-secondary:hover {
  background-color: var(--bg-hover);
}

.btn-cancel {
  background: none;
  color: var(--text-secondary);
  border-color: var(--border-color);
}

.btn-cancel:hover {
  background-color: var(--bg-hover);
}

.btn-danger {
  background-color: var(--destructive, #ef4444);
  color: #fff;
}

.btn-danger:hover:not(:disabled) {
  opacity: 0.9;
}

.dialog-icon-danger {
  color: var(--destructive, #ef4444);
}

/* 删除分支对话框：复选项 */
.delete-options {
  display: flex;
  flex-direction: column;
  gap: 10px;
  margin-top: 10px;
}

.delete-option {
  display: flex;
  align-items: center;
  gap: 8px;
  cursor: pointer;
  font-size: 12px;
  color: var(--text-primary);
  user-select: none;
}

.delete-checkbox {
  -webkit-appearance: none;
  appearance: none;
  width: 15px;
  height: 15px;
  flex-shrink: 0;
  margin: 0;
  border: 1px solid var(--border-medium);
  border-radius: 3px;
  background-color: var(--bg-primary);
  cursor: pointer;
  position: relative;
  transition: background-color 0.15s, border-color 0.15s;
}

.delete-checkbox:hover {
  border-color: var(--accent-primary);
}

.delete-checkbox:checked {
  background-color: var(--accent-primary);
  border-color: var(--accent-primary);
}

.delete-checkbox:checked::after {
  content: '';
  position: absolute;
  left: 4px;
  top: 1px;
  width: 4px;
  height: 8px;
  border: solid #fff;
  border-width: 0 2px 2px 0;
  transform: rotate(45deg);
}

.delete-option-label {
  line-height: 1.4;
}

/* 不可删除状态：整个 label 变灰 + 不可点击 */
.delete-option-disabled {
  cursor: not-allowed !important;
  color: var(--text-muted, #94a3b8);
}

.delete-option-disabled .delete-checkbox {
  cursor: not-allowed;
  opacity: 0.5;
}

.delete-option-disabled .delete-checkbox:hover {
  border-color: var(--border-medium);
}

.delete-option-disabled .delete-checkbox:checked {
  background-color: var(--border-medium, #cbd5e1);
  border-color: var(--border-medium, #cbd5e1);
}

/* 「不可删除」内联标签，与主文本用更弱的颜色区分 */
.delete-option-tag {
  margin-left: 6px;
  font-size: 11px;
  color: var(--text-muted, #94a3b8);
  font-weight: normal;
}

/* 检测中：整个 label 中性灰，checkbox 由 :disabled 自动灰显，cursor 等待感 */
.delete-option-checking {
  cursor: progress !important;
  color: var(--text-muted, #94a3b8);
}

.delete-option-checking .delete-option-tag {
  color: var(--accent-primary, #3b82f6);
}

.delete-option-checking .delete-checkbox {
  cursor: progress;
}

.dialog-hint-sub {
  margin-top: 8px !important;
}

@keyframes fadeIn {
  from { opacity: 0; }
  to { opacity: 1; }
}

@keyframes dialogIn {
  from {
    opacity: 0;
    transform: translateY(-10px) scale(0.98);
  }
  to {
    opacity: 1;
    transform: translateY(0) scale(1);
  }
}

/* ===== 储藏项样式 ===== */
.stash-item {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 10px;
  border-radius: 6px;
  transition: background-color 0.15s;
}

.stash-item:hover {
  background-color: var(--bg-hover);
}

.stash-icon {
  flex-shrink: 0;
  color: var(--text-secondary);
}

.stash-label {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
  color: var(--text-primary);
}

.stash-branch {
  flex-shrink: 0;
  font-size: 11px;
  padding: 1px 6px;
  border-radius: 4px;
  background-color: var(--bg-code, rgba(100, 116, 139, 0.15));
  color: var(--text-secondary);
  max-width: 80px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.stash-actions {
  display: flex;
  gap: 2px;
  flex-shrink: 0;
  opacity: 0;
  transition: opacity 0.15s;
}

.stash-item:hover .stash-actions {
  opacity: 1;
}

.stash-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 22px;
  height: 22px;
  border: none;
  border-radius: 4px;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  transition: background-color 0.15s, color 0.15s;
}

.stash-btn:hover {
  background-color: var(--bg-active);
  color: var(--brand-primary);
}

.stash-btn-danger:hover {
  color: var(--destructive, #ef4444);
}
</style>
