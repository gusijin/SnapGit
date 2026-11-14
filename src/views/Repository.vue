<script setup lang="ts">
import { useRepository } from '../composables/useRepository'
import MenuBar from '../components/MenuBar.vue'
import TitleBar from '../components/TitleBar.vue'
import ToolBar from '../components/ToolBar.vue'
import RepositoryList from '../components/RepositoryList.vue'
import BranchPanel from '../components/BranchPanel.vue'
import SubmodulePanel from '../components/SubmodulePanel.vue'
import RepositoryWorkspace from '../components/repository/RepositoryWorkspace.vue'
import PushDialog from '../components/PushDialog.vue'
import CloneDialog from '../components/CloneDialog.vue'
import RepoConfigDialog from '../components/RepoConfigDialog.vue'
import CheckoutBranchDialog from '../components/CheckoutBranchDialog.vue'
import AddRemoteDialog from '../components/AddRemoteDialog.vue'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'
import { Label } from '@/components/ui/label'
import { GitCommitVertical, ArrowUpFromLine, FolderOpen, HelpCircle, XCircle } from 'lucide-vue-next'

const {
  isMacOS, repoPath, currentBranch, commits, loadingMoreCommits, noMoreCommits, branches, branchListReady,
  fileStatuses, fileStatusLoading, stashes, upstreamBranch, remoteUrl, lastPullTime, scannedProjects,
  isScanning, fileTrees, loadingFileTree, switchingPath, switchingShown, selectedFiles, selectedFileDiff,
  primarySelectedFile, firstScreenReady, diffLoading, diffAutoScroll, fileViewMode, mainView, currentCommitId,
  commitFiles, displayedFiles, workingTreeCounts, repoName, leftPanelWidth, branchPanelFlex, fileListFlex,
  diffAreaFlex, logAreaFlex, logPaneFlex, logDetailFlex, leftPanelCollapsed, logCollapsed, showCommitDialog,
  commitMessage, committingFiles, isCommitting, showDeleteDialog, projectToDelete, checkoutError,
  checkoutTargetBranch, showStashDialog, stashMessage, stashIncludeUntracked, stashKeepIndex, isStashing,
  toastMessage, toastType, toastVisible, errorDialogVisible, errorMessage, showPushDialog, pushDialogBranch,
  pushDialogAuthMode, pushDialogError, showCloneDialog, showRepoConfig, configRepoPath, showCheckoutBranch,
  branchPanelRef, submodulePanelRef, showSubmodulePanel, showInitRepoDialog, pendingInitPath, initing,
  showNoRemoteDialog, showPullDivergedDialog, pullConflictActive, pendingPullStrategy,
  showAddRemoteDialog, showWelcomeDialog, isPulling, isPushing, statusBarMode, statusBarText,
  handleOpenRepo, handleCommitFromToolbar, handlePush, handlePull, refreshAll, handleCheckoutBranch,
  handleMenuAction, openScannedProject, requestDeleteProject, openRepoConfig, loadProjectFileTree,
  handleSelectFileFromTree, handleCheckoutFastForward, handleCheckoutForce, handleCheckoutCancel, handleCreateBranch,
  handleMergeBranch, handleRenameBranch, handleDeleteBranch, handleDeleteRemoteBranch, handlePushBranch,
  handleCheckoutRemote, handleStashApply, handleStashDrop, onSubmoduleChanged, openSubmodule, handleSelectFiles,
  handleStageFiles, requestCommitFiles, handleDiscardFiles, handleOpenFile, finishPullResolve, abortPullResolve,
  loadMoreCommits, handleSelectCommit, openCommitHistory, exitLogView, startResizeCenter, startResizeLogView,
  startResizeLogDetail, startResizeBranchPanel, startResizeLeftPanel, cancelCommit, handleCommitAndPush, handleCommit,
  cancelDeleteProject, confirmDeleteProject, confirmInitRepo, openAddRemoteDialog, handlePullStrategy, onRemoteAdded,
  confirmStash, autoScanProjects, handlePushed, handleCloned, handlePullConflict, t,
} = useRepository()
</script>

<template>
  <div class="app-container" :class="{ 'is-macos': isMacOS }">

    <!-- 顶部自定义标题栏（Windows/Linux 跟随主题，macOS 保留系统标题栏） -->
    <TitleBar :subtitle="repoName" />

    <!-- 顶部菜单栏（macOS 使用系统原生菜单栏，不显示窗口内菜单栏） -->
    <MenuBar
      v-if="!isMacOS"
      @open-repo="handleOpenRepo"
      @clone-repo="showCloneDialog = true"
      @commit="handleCommitFromToolbar"
      @push="handlePush"
      @pull="handlePull"
      @refresh="refreshAll"
      @branch="handleMenuAction('new-branch')"
      @checkout-branch="handleMenuAction('checkout-branch')"
      @merge="handleMenuAction('merge')"
      @repo-config="handleMenuAction('repo-config')"
      :submodule-visible="showSubmodulePanel"
      :sidebar-collapsed="leftPanelCollapsed"
      :current-view="mainView"
      @toggle-submodules="handleMenuAction('toggle-submodules')"
      @toggle-sidebar="handleMenuAction('toggle-sidebar')"
      @show-working-tree="handleMenuAction('show-working-tree')"
      @show-log="handleMenuAction('show-log')"
    />

    <!-- 工具栏 -->
    <ToolBar
      :sidebar-collapsed="leftPanelCollapsed"
      :current-branch="currentBranch"
      :branch-list-ready="branchListReady"
      :repository-path="repoPath"
      :branches="branches"
      :upstream="upstreamBranch"
      :working-tree="workingTreeCounts"
      :remote-url="remoteUrl"
      :repo-name="repoName"
      :last-pull-time="lastPullTime"
      :pulling="isPulling"
      :pushing="isPushing"
      @open-repo="handleOpenRepo"
      @commit="handleCommitFromToolbar"
      @push="handlePush"
      @pull="handlePull"
      @refresh="refreshAll"
      @checkout-branch="handleCheckoutBranch"
      @branch="handleMenuAction('new-branch')"
      :current-view="mainView"
      @stash="handleMenuAction('stash')"
      @show-working-tree="handleMenuAction('show-working-tree')"
      @show-log="handleMenuAction('show-log')"
      @toggle-sidebar="handleMenuAction('toggle-sidebar')"
    />

    <!-- 主布局 -->
    <div class="main-layout">
      <!-- 左侧面板（折叠时宽度切 0，中间面板自动撑满，参考 VS Code / GitKraken 侧边栏） -->
      <aside class="left-panel" :class="{ 'is-collapsed': leftPanelCollapsed }" :style="{ width: leftPanelCollapsed ? '0px' : leftPanelWidth + 'px' }">
        <RepositoryList 
          :projects="scannedProjects"
          :current-path="repoPath"
          :is-scanning="isScanning"
          :file-trees="fileTrees"
          :file-statuses="fileStatuses"
          :loading-file-tree="loadingFileTree"
          :switching-path="switchingPath"
          :switching-shown="switchingShown"
          @select-project="openScannedProject"
          @delete-project="requestDeleteProject"
          @config-project="openRepoConfig"
          @load-file-tree="loadProjectFileTree"
          @select-file="handleSelectFileFromTree"
        />
        
        <div v-if="repoPath" class="resize-handle" @mousedown="startResizeBranchPanel"></div>
        
        <BranchPanel 
          v-if="repoPath"
          ref="branchPanelRef"
          :repo-path="repoPath"
          :branches="branches"
          :current-branch="currentBranch"
          :checkout-error="checkoutError"
          :checkout-target-branch="checkoutTargetBranch"
          :stashes="stashes"
          :style="{ flex: branchPanelFlex }"
          @checkout-branch="handleCheckoutBranch"
          @checkout-fast-forward="handleCheckoutFastForward"
          @checkout-force="handleCheckoutForce"
          @checkout-cancel="handleCheckoutCancel"
          @create-branch="handleCreateBranch"
          @merge-branch="handleMergeBranch"
          @rename-branch="handleRenameBranch"
          @delete-branch="handleDeleteBranch"
          @delete-remote-branch="handleDeleteRemoteBranch"
          @push-branch="handlePushBranch"
          @pull-branch="handlePull"
          @checkout-remote="handleCheckoutRemote"
          @stash-apply="handleStashApply"
          @stash-drop="handleStashDrop"
        />

        <!-- 子模块面板：defer-load 让它保持挂载但推迟 list_submodules（多个 git 子进程 +
             逐子模块 status 扫描），等首屏（分支+提交日志）就绪后再拉，避免抢磁盘 IO。
             用 prop 而非 v-if：切库时不卸载/重建面板，无闪烁 -->
        <SubmodulePanel
          v-if="repoPath && showSubmodulePanel"
          ref="submodulePanelRef"
          :repo-path="repoPath"
          :defer-load="!firstScreenReady"
          @changed="onSubmoduleChanged"
          @open-submodule="openSubmodule"
          @added="onSubmoduleChanged"
        />
      </aside>

      <!-- 仓库面板与变更文件面板之间的左右拖拽分隔条（折叠时一并隐藏，避免孤立拖拽条） -->
      <div v-show="!leftPanelCollapsed" class="resize-handle-col" @mousedown="startResizeLeftPanel"></div>

      <RepositoryWorkspace
      :main-view="mainView"
      :file-list-flex="fileListFlex"
      :diff-area-flex="diffAreaFlex"
      :log-area-flex="logAreaFlex"
      :log-pane-flex="logPaneFlex"
      :log-detail-flex="logDetailFlex"
      :log-collapsed="logCollapsed"
      :commits="commits"
      :selected-commit-id="currentCommitId"
      :loading-more-commits="loadingMoreCommits"
      :no-more-commits="noMoreCommits"
      :displayed-files="displayedFiles"
      :commit-files="commitFiles"
      :selected-files="selectedFiles"
      :file-status-loading="fileStatusLoading"
      :pull-conflict-active="pullConflictActive"
      :pull-strategy="pendingPullStrategy"
      :selected-file-diff="selectedFileDiff"
      :primary-selected-file="primarySelectedFile"
      :diff-loading="diffLoading"
      :file-view-mode="fileViewMode"
      :diff-auto-scroll="diffAutoScroll"
      @update:log-collapsed="logCollapsed = $event"
      @resize-center="startResizeCenter"
      @resize-log-view="startResizeLogView"
      @resize-log-detail="startResizeLogDetail"
      @select-files="handleSelectFiles"
      @stage-files="handleStageFiles"
      @commit-files="requestCommitFiles"
      @discard-files="handleDiscardFiles"
      @open-file="handleOpenFile"
      @refresh="refreshAll"
      @resolve-conflicts="finishPullResolve"
      @abort-conflicts="abortPullResolve"
      @select-commit="handleSelectCommit"
      @open-history="openCommitHistory"
      @load-more="loadMoreCommits"
      @exit-log="exitLogView"
    />
      </div>

    <!-- 全局底部状态栏：根 flex 列的最后一项，与 mainView 无关，切到提交日志视图也不卸载。
         方案 A：严格一行文字 —— 空闲显示「仓库名 · 工作区摘要」，冲突整行转红，
         拉取时整行替换为进度文字；容器恒常驻 26px，文字变化零布局跳动。
         不加 key / out-in：拉取开始必须零延迟显示，进度每帧刷新也不该重播淡入淡出。 -->
    <footer class="status-bar">
      <Transition name="status-fade">
        <span
          v-if="statusBarText"
          class="status-bar-text"
          :class="{ 'is-alert': statusBarMode === 'alert' }"
          :title="statusBarText"
        >{{ statusBarText }}</span>
      </Transition>
    </footer>

    <!-- 提交对话框 -->
    <Dialog v-model:open="showCommitDialog">
      <DialogContent class="max-w-[460px] gap-4 p-5">
        <DialogHeader class="gap-3">
          <div class="flex items-center gap-3">
            <div class="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl border border-primary/20 bg-primary/15 text-primary">
              <GitCommitVertical class="h-5 w-5" />
            </div>
            <div class="flex min-w-0 flex-col gap-0.5">
              <DialogTitle class="text-base">{{ t('repository.commitDialogTitle') }}</DialogTitle>
              <DialogDescription v-if="committingFiles.length > 0" class="text-xs">
                <template v-if="committingFiles.length === 1">
                  {{ t('repository.commitFileOnly', { file: committingFiles[0].split(/[\\/]/).pop() }) }}
                </template>
                <template v-else-if="fileStatuses.filter(f => f.status !== 'clean').length === committingFiles.length && committingFiles.length > 1">
                  {{ t('repository.commitAllFiles', { n: committingFiles.length }) }}
                </template>
                <template v-else>
                  {{ t('repository.commitTotalFiles', { n: committingFiles.length }) }}
                </template>
              </DialogDescription>
            </div>
          </div>
        </DialogHeader>

        <div class="grid gap-2">
          <Label for="commit-message" class="text-xs font-medium text-muted-foreground">
            {{ t('repository.commitMessage') }}
          </Label>
          <Textarea
            id="commit-message"
            v-model="commitMessage"
            rows="4"
            class="resize-none text-sm"
            :placeholder="t('repository.commitPlaceholder')"
            @keydown.enter.ctrl="handleCommit"
          />
        </div>

        <div class="flex items-center justify-between gap-3 pt-1">
          <p class="whitespace-nowrap text-xs text-muted-foreground">{{ t('repository.commitShortcut') }}</p>
          <div class="flex shrink-0 gap-2">
            <Button variant="outline" size="sm" @click="cancelCommit" :disabled="isCommitting">{{ t('common.cancel') }}</Button>
            <Button variant="outline" size="sm" @click="handleCommitAndPush" :disabled="!commitMessage.trim() || isCommitting">
              <ArrowUpFromLine :size="14" />
              {{ t('repository.commitAndPush') }}
            </Button>
            <Button size="sm" :disabled="!commitMessage.trim() || isCommitting" @click="handleCommit">
              {{ isCommitting ? t('repository.committing') : t('repository.confirmCommit') }}
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>

    <!-- 删除确认对话框 -->
    <Dialog v-model:open="showDeleteDialog">
      <DialogContent class="max-w-[400px]">
        <DialogHeader>
          <DialogTitle>{{ t('repository.confirmDeleteTitle') }}</DialogTitle>
        </DialogHeader>
        <div class="space-y-2 py-1">
          <p class="text-sm text-foreground">
            {{ t('repository.confirmDeleteDesc', { name: projectToDelete?.name || '' }) }}
          </p>
          <p class="text-xs text-destructive bg-destructive/10 p-2 rounded">
            {{ t('repository.confirmDeleteWarning') }}
          </p>
          <p class="text-xs text-muted-foreground font-mono break-all">{{ projectToDelete?.path }}</p>
        </div>
        <DialogFooter>
          <Button variant="outline" @click="cancelDeleteProject">{{ t('common.cancel') }}</Button>
          <Button variant="destructive" @click="confirmDeleteProject">{{ t('repository.confirmDelete') }}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>

    <!-- 打开普通文件夹 → "是否初始化为新 Git 仓库"对话框 -->
    <Dialog v-model:open="showInitRepoDialog">
      <DialogContent class="max-w-[480px] gap-3 p-5">
        <div class="init-repo-row">
          <div class="init-repo-icon">
            <HelpCircle :size="28" />
          </div>
          <div class="init-repo-text">
            <DialogHeader class="gap-1 p-0">
              <DialogTitle class="text-[15px] leading-snug">
                {{ t('repository.initRepoTitle', { name: pendingInitPath.split(/[\\/]/).pop() || pendingInitPath }) }}
              </DialogTitle>
            </DialogHeader>
            <p class="init-repo-desc">{{ t('repository.initRepoDesc') }}</p>
          </div>
        </div>
        <DialogFooter class="gap-2">
          <Button variant="outline" :disabled="initing" @click="showInitRepoDialog = false">
            {{ t('repository.initRepoCancel') }}
          </Button>
          <Button :disabled="initing" @click="confirmInitRepo">
            {{ t('repository.initRepoConfirm') }}{{ initing ? '…' : '' }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>

    <!-- 拉取无远程 → "请先创建远程"对话框 -->
    <Dialog v-model:open="showNoRemoteDialog">
      <DialogContent class="max-w-[480px] gap-3 p-5">
        <div class="init-repo-row">
          <div class="init-repo-icon">
            <HelpCircle :size="28" />
          </div>
          <div class="init-repo-text">
            <DialogHeader class="gap-1 p-0">
              <DialogTitle class="text-[15px] leading-snug">
                {{ t('repository.pullNoRemoteTitle') }}
              </DialogTitle>
            </DialogHeader>
            <p class="init-repo-desc">{{ t('repository.pullNoRemoteDesc') }}</p>
          </div>
        </div>
        <DialogFooter class="gap-2">
          <Button variant="outline" @click="showNoRemoteDialog = false">
            {{ t('repository.pullNoRemoteCancel') }}
          </Button>
          <Button @click="openAddRemoteDialog">
            {{ t('repository.pullNoRemoteCreate') }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>

    <!-- 分支分叉 → "选择拉取方式"对话框 -->
    <Dialog v-model:open="showPullDivergedDialog">
      <DialogContent class="max-w-[460px] gap-3 p-5">
        <DialogHeader class="gap-1 p-0">
          <DialogTitle class="text-[15px] leading-snug">
            {{ t('repository.pullDivergedTitle') }}
          </DialogTitle>
          <p class="init-repo-desc">{{ t('repository.pullDivergedDesc') }}</p>
        </DialogHeader>
        <div class="mt-1 flex flex-col gap-2">
          <button
            type="button"
            class="flex w-full flex-col items-start gap-1 rounded-lg border border-[var(--border-medium)] bg-[var(--bg-secondary)] px-3 py-2.5 text-left transition-colors hover:border-[var(--brand-primary)] hover:bg-[var(--bg-hover)]"
            @click="handlePullStrategy('merge')"
          >
            <span class="text-[13px] font-medium text-[var(--text-bright)]">{{ t('repository.pullMerge') }}</span>
            <span class="text-[12px] leading-snug text-[var(--text-tertiary)]">{{ t('repository.pullMergeHint') }}</span>
          </button>
          <button
            type="button"
            class="flex w-full flex-col items-start gap-1 rounded-lg border border-[var(--border-medium)] bg-[var(--bg-secondary)] px-3 py-2.5 text-left transition-colors hover:border-[var(--brand-primary)] hover:bg-[var(--bg-hover)]"
            @click="handlePullStrategy('rebase')"
          >
            <span class="text-[13px] font-medium text-[var(--text-bright)]">{{ t('repository.pullRebase') }}</span>
            <span class="text-[12px] leading-snug text-[var(--text-tertiary)]">{{ t('repository.pullRebaseHint') }}</span>
          </button>
        </div>
        <DialogFooter class="gap-2">
          <Button variant="outline" @click="showPullDivergedDialog = false">
            {{ t('repository.pullDivergedCancel') }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>

    <!-- 「创建远程」：添加远程仓库对话框 -->
    <AddRemoteDialog
      :open="showAddRemoteDialog"
      :repo-path="repoPath"
      @update:open="(v: boolean) => showAddRemoteDialog = v"
      @added="onRemoteAdded"
    />

    <!-- 推送对话框 -->
    <Teleport to="body">
      <PushDialog
        v-if="showPushDialog && repoPath"
        :repo-path="repoPath"
        :branches="branches"
        :current-branch="currentBranch"
        :initial-branch="pushDialogBranch || undefined"
        :initial-auth-mode="pushDialogAuthMode"
        :initial-error="pushDialogError || undefined"
        @close="showPushDialog = false"
        @pushed="handlePushed"
        @pushing="(v: boolean) => { isPushing = v }"
        @pull-conflict="handlePullConflict"
      />
    </Teleport>

    <!-- 克隆仓库对话框 -->
    <Teleport to="body">
      <CloneDialog
        v-if="showCloneDialog"
        @close="showCloneDialog = false"
        @cloned="handleCloned"
      />
    </Teleport>

    <!-- 仓库配置对话框 -->
    <Teleport to="body">
      <RepoConfigDialog
        v-model:open="showRepoConfig"
        :repo-path="configRepoPath"
      />
    </Teleport>

    <!-- 切换分支对话框 -->
    <Teleport to="body">
      <CheckoutBranchDialog
        v-model:open="showCheckoutBranch"
        :branches="branches"
        :current-branch="currentBranch"
        @checkout="(name: string) => handleCheckoutBranch(name)"
      />
    </Teleport>

    <!-- 储藏对话框 -->
    <Teleport to="body">
      <div v-if="showStashDialog" class="dialog-mask" @click="showStashDialog = false">
        <div class="dialog" @click.stop>
          <div class="dialog-header">{{ t('repository.stashDialogTitle') }}</div>
          <div class="dialog-body">
            <div class="stash-desc">
              <p>{{ t('repository.stashDialogDesc') }}</p>
              <p class="stash-hint">{{ t('repository.stashDialogHint') }}</p>
            </div>
            <div class="dialog-message">
              <label class="stash-label" for="stash-msg">{{ t('repository.stashMessage') }}</label>
              <textarea
                id="stash-msg"
                v-model="stashMessage"
                class="input-field textarea-field"
                :placeholder="t('repository.stashPlaceholder')"
                rows="3"
                autofocus
              />
            </div>
            <div class="stash-options">
              <label class="stash-option">
                <input type="checkbox" v-model="stashIncludeUntracked" />
                <span>{{ t('repository.stashIncludeUntracked') }}</span>
              </label>
              <label class="stash-option">
                <input type="checkbox" v-model="stashKeepIndex" />
                <span>{{ t('repository.stashKeepIndex') }}</span>
              </label>
            </div>
          </div>
          <div class="dialog-footer">
            <button class="btn btn-cancel" :disabled="isStashing" @click="showStashDialog = false">{{ t('common.cancel') }}</button>
            <button class="btn btn-primary" :disabled="isStashing" @click="confirmStash">
              {{ isStashing ? t('repository.stashing') : t('repository.stashAll') }}
            </button>
          </div>
        </div>
      </div>
    </Teleport>

    <!-- 无仓库时的欢迎弹窗 -->
    <Dialog v-model:open="showWelcomeDialog">
      <DialogContent
        class="welcome-dialog-content max-w-[420px] gap-5 p-6"
        @interact-outside.prevent
        @escape-key-down.prevent
      >
        <DialogHeader class="items-center gap-3 p-0 text-center">
          <div class="flex h-16 w-16 items-center justify-center rounded-2xl border border-primary/20 bg-primary/10 text-primary">
            <FolderOpen :size="36" />
          </div>
          <div class="flex flex-col gap-1">
            <DialogTitle class="text-lg">{{ t('repository.emptyTitle') }}</DialogTitle>
            <DialogDescription class="text-sm text-muted-foreground">
              {{ t('repository.emptySubtitle') }}
            </DialogDescription>
          </div>
        </DialogHeader>
        <div class="flex flex-col gap-2 pt-1">
          <Button class="w-full gap-2" @click="handleOpenRepo">
            <FolderOpen :size="16" />
            {{ t('repository.openRepoBtn') }}
          </Button>
          <Button variant="outline" class="w-full gap-2" :disabled="isScanning" @click="autoScanProjects">
            <svg v-if="isScanning" class="mr-1 h-4 w-4 animate-spin" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
              <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
              <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
            </svg>
            <span v-else>🔄</span>
            {{ t('repository.rescanBtn') }}
          </Button>
        </div>
      </DialogContent>
    </Dialog>

    <!-- Toast 提示（成功等轻量反馈） -->
    <Teleport to="body">
      <Transition name="toast">
        <div v-if="toastVisible" class="toast" :class="toastType">
          <span class="toast-icon">{{ toastType === 'success' ? '✓' : '✗' }}</span>
          <span class="toast-msg">{{ toastMessage }}</span>
        </div>
      </Transition>
    </Teleport>

    <!-- 错误提示对话框：持久弹窗，需用户点击「关闭」才消失（不会自动消失） -->
    <Dialog v-model:open="errorDialogVisible">
      <DialogContent class="max-w-[440px] gap-3 p-5">
        <DialogHeader class="gap-2 p-0">
          <div class="flex items-center gap-2">
            <div class="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg border border-[var(--color-del)]/30 bg-[var(--color-del)]/10 text-[var(--color-del)]">
              <XCircle class="h-4 w-4" />
            </div>
            <DialogTitle class="text-[15px] leading-snug">{{ t('repository.errorTitle') }}</DialogTitle>
          </div>
        </DialogHeader>
        <p class="whitespace-pre-wrap break-words text-sm leading-relaxed text-[var(--text-muted)]">{{ errorMessage }}</p>
        <DialogFooter class="gap-2">
          <Button variant="outline" @click="errorDialogVisible = false">{{ t('repository.errorClose') }}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </div>
</template>

<style src="../styles/repository.css" scoped></style>
