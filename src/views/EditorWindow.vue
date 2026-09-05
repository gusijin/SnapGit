<script setup lang="ts">
import { ref, onMounted, computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { getCurrentWindow } from '@tauri-apps/api/window'
import ConflictSolver from '../components/ConflictSolver.vue'
import DiffEditor from '../components/DiffEditor.vue'
import TitleBar from '../components/TitleBar.vue'
import { getConflictFile, getFileDiff, openEditWindow, closeEditWindow, getEditArgs } from '../api/git'
import type { ConflictFile, FileDiff } from '../types'

interface EditArgs {
  repoPath: string
  filePath: string
  mode: 'conflict' | 'diff'
  // 修改文件列表（用于在编辑窗口内做「上一个/下一个文件」跳转）
  // 单文件打开时不传，按钮自动 disabled
  filesList?: string[]
  // 当前编辑文件在 filesList 中的下标
  currentIndex?: number
}

// 参数状态：先从 window 变量取（initialization_script 加速路径），
// 没有则调用 Rust get_edit_args() 获取（带重试）。
const args = ref<EditArgs | null>(null)

const repoPath = computed(() => args.value?.repoPath || '')
const filePath = computed(() => args.value?.filePath || '')
const mode = computed(() => (args.value?.mode === 'conflict' ? 'conflict' : 'diff'))
const filesList = computed(() => args.value?.filesList || [])
const currentIndex = computed(() => args.value?.currentIndex ?? -1)

const { t } = useI18n()
const conflictFile = ref<ConflictFile | null>(null)
const diff = ref<FileDiff | null>(null)
const error = ref('')
const loading = ref(true)

// 获取当前窗口 label（用于显式传递给 Rust 命令）
function getWindowLabel(): string {
  try {
    return getCurrentWindow().label || ''
  } catch {
    const internals = (window as unknown as { __TAURI_INTERNALS__?: { metadata?: { currentWindow?: { label?: string } } } }).__TAURI_INTERNALS__
    return internals?.metadata?.currentWindow?.label || ''
  }
}
const windowLabel = getWindowLabel()

function sleep(ms: number) {
  return new Promise(resolve => setTimeout(resolve, ms))
}

async function resolveArgs(): Promise<boolean> {
  // 1. 优先用 initialization_script 注入的变量（最快）
  const scriptArgs = (window as unknown as { __SNAPGIT_EDIT_ARGS__?: EditArgs }).__SNAPGIT_EDIT_ARGS__
  if (scriptArgs?.repoPath && scriptArgs?.filePath) {
    args.value = scriptArgs
    return true
  }

  // 2. 调用 Rust 命令取参数，带重试（治 dev 模式 script 注入失效 + 窗口初始化延迟）
  const label = windowLabel || undefined
  const maxRetries = 3
  for (let attempt = 1; attempt <= maxRetries; attempt++) {
    try {
      const remoteArgs = await getEditArgs(label)
      if (remoteArgs?.repoPath && remoteArgs?.filePath) {
        args.value = remoteArgs
        // 写回 window，方便子组件或后续操作直接读
        ;(window as unknown as { __SNAPGIT_EDIT_ARGS__?: EditArgs }).__SNAPGIT_EDIT_ARGS__ = remoteArgs
        return true
      }
    } catch (e) {
      if (attempt === maxRetries) {
        error.value = typeof e === 'string' ? e : (e as any)?.toString?.() || String(e)
      } else {
        // 重试前等待，给窗口更多时间完成初始化
        await sleep(300 * attempt)
      }
    }
  }
  return false
}

onMounted(async () => {
  const ok = await resolveArgs()
  if (!ok) {
    loading.value = false
    error.value = error.value || t('editorWindow.noArgs')
    return
  }
  try {
    if (mode.value === 'conflict') {
      conflictFile.value = await getConflictFile(repoPath.value, filePath.value)
    } else {
      diff.value = await getFileDiff(repoPath.value, filePath.value)
    }
  } catch (e) {
    console.error('[EditorWindow] 加载文件失败:', e)
    error.value = typeof e === 'string' ? e : (e as any)?.toString?.() || String(e)
  } finally {
    loading.value = false
  }
})

async function handleSaved() {
  // 保存成功后：通知主窗口刷新 + 关闭本窗口
  try {
    await closeEditWindow()
  } catch (e) {
    console.error('Close edit window error:', e)
  }
}

// 「上一个/下一个文件」：Tauri 没有"修改当前窗口文件"的 API，
// 关闭当前窗口 + 重新打开目标文件，新窗口的 args 由 Rust 重新注入。
async function handleNavigateFile(nextIndex: number) {
  const list = filesList.value
  if (nextIndex < 0 || nextIndex >= list.length) return
  const targetPath = list[nextIndex]
  try {
    await openEditWindow(repoPath.value, targetPath, 'diff', list, nextIndex)
    await closeEditWindow()
  } catch (e) {
    console.error('Navigate file error:', e)
    alert(t('editorWindow.navigateFailed', { error: (typeof e === 'string' ? e : (e as any)?.toString?.() || String(e)) }))
  }
}

// 「重新加载」：从 Rust 重新拉当前文件的 diff，丢弃右侧编辑区未保存内容
async function handleReloadDiff() {
  if (!repoPath.value || !filePath.value) return
  loading.value = true
  try {
    diff.value = await getFileDiff(repoPath.value, filePath.value)
    error.value = ''
  } catch (e) {
    console.error('[EditorWindow] 重新加载 diff 失败:', e)
    error.value = typeof e === 'string' ? e : (e as any)?.toString?.() || String(e)
  } finally {
    loading.value = false
  }
}

</script>

<template>
  <div class="editor-window">
    <!-- 顶部自定义标题栏（Windows/Linux 跟随主题，macOS 保留系统标题栏） -->
    <TitleBar :subtitle="filePath || repoPath" />

    <div v-if="error" class="editor-error">
      <p>{{ t('editorWindow.loadFailed') }}</p>
      <p class="error-detail">{{ error }}</p>
      <button class="btn btn-primary" @click="closeEditWindow">{{ t('editorWindow.close') }}</button>
    </div>

    <template v-else>
      <ConflictSolver
        v-if="mode === 'conflict' && conflictFile"
        :conflict-file="conflictFile"
        :repo-path="repoPath"
        @save="handleSaved"
        @close="closeEditWindow"
      />
      <DiffEditor
        v-else-if="mode === 'diff' && diff"
        :repo-path="repoPath"
        :file-path="filePath"
        :diff="diff"
        :files-list="filesList"
        :current-index="currentIndex"
        @save="handleSaved"
        @close="closeEditWindow"
        @navigate-file="handleNavigateFile"
        @reload="handleReloadDiff"
      />
      <div v-else class="editor-loading">{{ t('editorWindow.loading') }}</div>
    </template>
  </div>
</template>

<style scoped>
.editor-window {
  display: flex;
  flex-direction: column;
  height: 100vh;
  min-height: 0;
  background-color: var(--bg-primary);
}

/* 顶部标题栏：显示当前编辑文件，附带关闭按钮 */
.editor-window-header {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 6px 12px;
  background-color: var(--bg-toolbar);
  border-bottom: 1px solid var(--border-color);
  font-size: 12px;
  flex-shrink: 0;
  -webkit-user-select: none;
  user-select: none;
}

.header-info {
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 0;
  flex: 1;
}

.header-close-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  border: none;
  border-radius: 6px;
  background: transparent;
  color: var(--text-tertiary);
  cursor: pointer;
  flex-shrink: 0;
  transition: background-color 0.1s, color 0.1s;
}

.header-close-btn:hover {
  background-color: var(--bg-active);
  color: var(--text-bright);
}

.window-title {
  font-weight: 600;
  color: var(--text-primary);
}

.window-path {
  font-family: Consolas, Monaco, monospace;
  font-size: 11px;
  color: var(--text-tertiary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.editor-window :deep(.conflict-solver),
.editor-window :deep(.diff-editor) {
  flex: 1;
  min-height: 0;
}

.editor-loading {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-muted);
  font-size: 13px;
}

.editor-error {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  color: var(--danger-color);
  font-size: 13px;
}

.error-detail {
  font-size: 11px;
  color: var(--text-tertiary);
  max-width: 70%;
  text-align: center;
  word-break: break-all;
}

.editor-error button {
  margin-top: 8px;
}

</style>