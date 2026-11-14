<script setup lang="ts">
import type { Commit, FileDiff, FileStatus } from '../../types'
import FileList from '../FileList.vue'
import DiffViewer from '../DiffViewer.vue'
import LogView from '../LogView.vue'

defineProps<{
  mainView: 'worktree' | 'log'
  fileListFlex: number
  diffAreaFlex: number
  logAreaFlex: number
  logPaneFlex: number
  logDetailFlex: number
  logCollapsed: boolean
  commits: Commit[]
  selectedCommitId: string
  loadingMoreCommits: boolean
  noMoreCommits: boolean
  displayedFiles: FileStatus[]
  commitFiles: FileStatus[]
  selectedFiles: string[]
  fileStatusLoading: boolean
  pullConflictActive: boolean
  pullStrategy: 'merge' | 'rebase'
  selectedFileDiff: FileDiff | null
  primarySelectedFile: string | null
  diffLoading: boolean
  fileViewMode: 'working-tree' | 'commit'
  diffAutoScroll: boolean
}>()

const emit = defineEmits<{
  (event: 'update:logCollapsed', value: boolean): void
  (eventName: 'resize-center', event: MouseEvent, target: 'file-diff' | 'diff-log'): void
  (eventName: 'resize-log-view', event: MouseEvent): void
  (eventName: 'resize-log-detail', event: MouseEvent): void
  (event: 'select-files', paths: string[]): void
  (event: 'stage-files', paths: string[]): void
  (event: 'commit-files', paths: string[]): void
  (event: 'discard-files', paths: string[]): void
  (event: 'open-file', file: FileStatus): void
  (event: 'refresh'): void
  (event: 'resolve-conflicts'): void
  (event: 'abort-conflicts'): void
  (event: 'select-commit', id: string): void
  (event: 'open-history', id: string): void
  (event: 'load-more'): void
  (event: 'exit-log'): void
}>()

const updateCollapsed = (value: boolean) => emit('update:logCollapsed', value)
</script>

<template>
  <section class="center-panel">
        <!-- 工作区视图（默认三段式：文件列表 / 差异 / 提交日志） -->
        <template v-if="mainView === 'worktree'">
          <!-- 上部：修改的文件列表 -->
          <div class="file-list-area" :style="{ flex: fileListFlex }">
            <FileList 
              :files="displayedFiles"
              :selected-files="selectedFiles"
              :view-mode="fileViewMode"
              :loading="fileStatusLoading"
              :conflict-active="pullConflictActive"
              :pull-strategy="pullStrategy"
              @select-files="emit('select-files', $event)"
              @stage-files="emit('stage-files', $event)"
              @commit-files="emit('commit-files', $event)"
              @discard-files="emit('discard-files', $event)"
              @open-file="emit('open-file', $event)"
              @refresh="emit('refresh')"
              @resolve-conflicts="emit('resolve-conflicts')"
              @abort-conflicts="emit('abort-conflicts')"
            />
          </div>
          <!-- 拖拉分隔条 -->
          <div class="resize-handle" @mousedown="emit('resize-center', $event, 'file-diff')"></div>
          <!-- 中部：差异查看器 -->
          <div class="diff-area" :style="{ flex: diffAreaFlex }">
            <DiffViewer 
              :diff="selectedFileDiff"
              :file-path="primarySelectedFile || ''"
              :loading="diffLoading"
              :view-mode="fileViewMode"
              :auto-scroll="diffAutoScroll"
            />
          </div>
          <!-- 拖拉分隔条 -->
          <div class="resize-handle" @mousedown="emit('resize-center', $event, 'diff-log')"></div>
          <!-- 下部：提交日志 -->
          <div class="log-area" :style="{ flex: logCollapsed ? '0 0 30px' : logAreaFlex }">
            <LogView 
              :collapsed="logCollapsed" @update:collapsed="updateCollapsed"
              :commits="commits"
              :selected-id="selectedCommitId"
              :loading-more="loadingMoreCommits"
              :no-more="noMoreCommits"
              :disable-single-click="true"
              @select-commit="emit('select-commit', $event)"
              @open-history="emit('open-history', $event)"
              @load-more="emit('load-more')"
            />
          </div>
        </template>

        <!-- 提交历史视图：左侧日志 / 右侧提交详情（文件 + 差异） -->
        <template v-else>
          <div class="log-view-layout">
            <div class="log-pane" :style="{ flex: logCollapsed ? '0 0 32px' : logPaneFlex }">
              <LogView 
                :collapsed="logCollapsed" @update:collapsed="updateCollapsed"
                :collapse-to-side="true"
                :commits="commits"
                :selected-id="selectedCommitId"
                :loading-more="loadingMoreCommits"
                :no-more="noMoreCommits"
              @select-commit="emit('select-commit', $event)"
              @open-history="emit('open-history', $event)"
              @load-more="emit('load-more')"
              :show-exit-button="true"
              @exit-log="emit('exit-log')"
            />
            </div>
            <!-- 提交日志面板 ↔ 提交文件面板 左右拖拽分隔条 -->
            <div class="resize-handle-col" @mousedown="emit('resize-log-view', $event)"></div>
            <!-- 右侧：提交详情（复用现有组件与 diff 链路） -->
            <div class="commit-detail-area" :style="{ flex: logDetailFlex }">
              <div class="file-list-area" :style="{ flex: fileListFlex }">
                <FileList 
                  :files="commitFiles"
                  :selected-files="selectedFiles"
                  view-mode="commit"
                  @select-files="emit('select-files', $event)"
                  @open-file="emit('open-file', $event)"
                  @refresh="emit('refresh')"
                />
              </div>
              <div class="resize-handle" @mousedown="emit('resize-log-detail', $event)"></div>
              <div class="diff-area" :style="{ flex: diffAreaFlex }">
                <DiffViewer 
                  :diff="selectedFileDiff"
                  :file-path="primarySelectedFile || ''"
                  :loading="diffLoading"
                  view-mode="commit"
                  :auto-scroll="diffAutoScroll"
                />
              </div>
            </div>
          </div>
        </template>
      </section>
</template>

<style src="./RepositoryWorkspace.css" scoped></style>
