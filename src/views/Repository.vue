<script setup lang="ts">
import { ref, computed, watch, onMounted, onBeforeUnmount } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  getCommits, getBranches, getFileStatus, stageFile, discardFile, commitChanges,
  checkoutBranch, pullBranch, openRepository, initRepository, saveRecentRepository, openFolderDialog,
  scanProjects, getFileTree, getFileDiff, getCommitFiles,
  createBranch, mergeBranch, renameBranch, checkoutRemoteBranch,
  getUpstream, getRemotes, getRemoteUrl, pushChanges, deleteBranch, deleteRemoteBranch,
  stashCreate, stashList, stashApply, stashDrop, openEditWindow,
} from '../api/git'
import type { Commit, Branch, FileStatus, ScannedProject, FileTreeNode, FileDiff, StashEntry } from '../types'
import { useTheme } from '../stores/theme'
import MenuBar from '../components/MenuBar.vue'
import TitleBar from '../components/TitleBar.vue'
import ToolBar from '../components/ToolBar.vue'
import RepositoryList from '../components/RepositoryList.vue'
import BranchPanel from '../components/BranchPanel.vue'
import FileList from '../components/FileList.vue'
import DiffViewer from '../components/DiffViewer.vue'
import LogView from '../components/LogView.vue'
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
import { GitCommitVertical, ArrowUpFromLine, FolderOpen, HelpCircle } from 'lucide-vue-next'

const { toggleTheme } = useTheme()
const { t } = useI18n()
const LAST_REPO_KEY = 'snapgit-last-repo'
const FIRST_LAUNCH_KEY = 'snapgit-first-launch'
const SCANNED_PROJECTS_KEY = 'snapgit-scanned-projects'

// 判断操作系统：macOS 使用原生菜单栏，隐藏窗口内菜单栏
// 优先使用 navigator.platform（更可靠），userAgent 作为后备
const isMacOS = /mac/i.test(navigator.platform) || /mac|darwin/i.test(navigator.userAgent)

// ===== 状态 =====
const repoPath = ref<string>('')
const currentBranch = ref<string>('')
const commits = ref<Commit[]>([])
// 提交日志分页：滚动到底自动加载更早的历史
const loadingMoreCommits = ref(false)
const noMoreCommits = ref(false)
const branches = ref<Branch[]>([])
const fileStatuses = ref<FileStatus[]>([])
const stashes = ref<StashEntry[]>([])

// 工作区状态弹窗所需数据
const upstreamBranch = ref<string | null>(null)
const remoteUrl = ref<string | null>(null)
const lastPullTime = ref<number | null>(null)

// 扫描状态
const scannedProjects = ref<ScannedProject[]>([])
const isScanning = ref(false)

// 文件树缓存
const fileTrees = ref<Record<string, FileTreeNode[]>>({})
const loadingFileTree = ref<string | null>(null)
// 仓库面板"切换中"指示：切换仓库超过 1 秒时，项目图标换成转圈，切换完成后恢复
const switchingPath = ref<string | null>(null)
const switchingShown = ref(false)
let switchTimer: ReturnType<typeof setTimeout> | null = null

// 差异
const selectedFiles = ref<string[]>([])
const selectedFileDiff = ref<FileDiff | null>(null)
const primarySelectedFile = computed(() => selectedFiles.value[0] || null)

// ===== 工作区文件自动刷新（fs watch） =====
// 监听仓库目录变化：外部修改/删除文件时自动刷新"修改的文件"面板
let unwatchFs: (() => void) | null = null
let fsRefreshTimer: ReturnType<typeof setTimeout> | null = null
let autoRefreshInFlight = false // 单飞标记：同一时刻只允许一次自动刷新在执行
let autoRefreshPending = false // 刷新执行期间的新触发合并为一次补跑
let diffRequestSeq = 0 // 防止旧的 diff 异步请求覆盖新结果
const diffLoading = ref(false) // 差异面板加载状态（大文件 diff 计算耗时）
let diffLoadingTimer: ReturnType<typeof setTimeout> | null = null
const diffAutoScroll = ref(true) // 用户主动切换文件时 true（滚动到变更行），自动刷新时 false（保持位置）

// 延迟显示加载状态：仅当 diff 计算超过 1 秒仍未完成时才展示，避免快速加载时 loading 闪烁
function startDiffLoading() {
  if (diffLoadingTimer) clearTimeout(diffLoadingTimer)
  diffLoadingTimer = setTimeout(() => {
    diffLoadingTimer = null
    diffLoading.value = true
  }, 1000)
}
function stopDiffLoading() {
  if (diffLoadingTimer) {
    clearTimeout(diffLoadingTimer)
    diffLoadingTimer = null
  }
  diffLoading.value = false
}

// 提交文件查看模式
const fileViewMode = ref<'working-tree' | 'commit'>('working-tree')
const mainView = ref<'worktree' | 'log'>('worktree')
const currentCommitId = ref<string>('')
const commitFiles = ref<FileStatus[]>([])
const displayedFiles = computed(() =>
  fileViewMode.value === 'commit' ? commitFiles.value : fileStatuses.value
)

// 工作区状态弹窗：工作树变更计数（与 FileList 的状态分类保持一致）
const workingTreeCounts = computed(() => {
  const counts = { modified: 0, untracked: 0, conflict: 0 }
  for (const f of fileStatuses.value) {
    if (f.status === 'conflict' || f.status === 'unmerged') counts.conflict++
    else if (f.status === 'untracked' || f.status === 'new') counts.untracked++
    else if (f.status !== 'clean') counts.modified++
  }
  return counts
})

// 仓库名（取路径最后一段）
const repoName = computed(() => {
  const p = repoPath.value
  if (!p) return ''
  const norm = p.replace(/[\\/]+$/, '')
  return norm.split(/[\\/]/).pop() || norm
})

// 分支面板大小
const branchPanelFlex = ref(1)
const isResizingBranchPanel = ref(false)
const BRANCH_PANEL_MIN_FLEX = 0.5
const BRANCH_PANEL_MAX_FLEX = 5.0
const REPO_LIST_FLEX = 1

// 中间面板大小
const fileListFlex = ref(1)
const diffAreaFlex = ref(1.4)
const logAreaFlex = ref(1)
const isResizingCenter = ref(false)
const CENTER_MIN_FLEX = 0.2

// 左侧面板（仓库面板）宽度 —— 与中间面板（变更文件）之间的左右拖拽
const leftPanelWidth = ref(320)
const isResizingLeftPanel = ref(false)
const LEFT_PANEL_MIN = 200
const LEFT_PANEL_MAX = 560

// 对话框
const showCommitDialog = ref(false)
const commitMessage = ref('')
const committingFiles = ref<string[]>([])
const isCommitting = ref(false)
const showDeleteDialog = ref(false)
const projectToDelete = ref<ScannedProject | null>(null)

// 文件编辑：改为在独立新窗口中打开（Rust 端 open_edit_window 创建 WebviewWindow）

// 分支切换错误对话框
const checkoutError = ref<string | null>(null)
const checkoutTargetBranch = ref('')

// ===== 分支面板拖动调整大小 =====
let resizeStartY = 0
let resizeStartFlex = 0
let leftPanelHeight = 0

function startResizeBranchPanel(e: MouseEvent) {
  if (e.button !== 0) return
  e.preventDefault()
  isResizingBranchPanel.value = true
  resizeStartY = e.clientY
  resizeStartFlex = branchPanelFlex.value
  
  const leftPanel = document.querySelector('.left-panel') as HTMLElement
  if (leftPanel) {
    leftPanelHeight = leftPanel.clientHeight
  }
  
  document.addEventListener('mousemove', onResizeBranchPanelMove)
  document.addEventListener('mouseup', stopResizeBranchPanel)
  document.body.style.cursor = 'ns-resize'
  document.body.style.userSelect = 'none'
  document.body.style.webkitUserSelect = 'none'
}

function onResizeBranchPanelMove(e: MouseEvent) {
  if (!isResizingBranchPanel.value) return
  const deltaY = resizeStartY - e.clientY
  const totalFlex = REPO_LIST_FLEX + resizeStartFlex
  const startBranchHeight = leftPanelHeight * (resizeStartFlex / totalFlex)
  const newBranchHeight = startBranchHeight + deltaY
  const newFlex = newBranchHeight / (leftPanelHeight - newBranchHeight)
  branchPanelFlex.value = Math.min(BRANCH_PANEL_MAX_FLEX, Math.max(BRANCH_PANEL_MIN_FLEX, newFlex))
}

function stopResizeBranchPanel() {
  isResizingBranchPanel.value = false
  document.removeEventListener('mousemove', onResizeBranchPanelMove)
  document.removeEventListener('mouseup', stopResizeBranchPanel)
  document.body.style.cursor = ''
  document.body.style.userSelect = ''
  document.body.style.webkitUserSelect = ''
}

// ===== 中间面板拖动调整大小 =====
let centerStartY = 0
let centerStartFlex1 = 0
let centerStartFlex2 = 0
let centerStartTotalFlex = 0
let centerPanelHeight = 0
let centerResizeTarget: 'file-diff' | 'diff-log' = 'file-diff'

function startResizeCenter(e: MouseEvent, target: 'file-diff' | 'diff-log') {
  if (e.button !== 0) return
  e.preventDefault()
  isResizingCenter.value = true
  centerResizeTarget = target
  centerStartY = e.clientY

  const centerPanel = document.querySelector('.center-panel') as HTMLElement
  if (centerPanel) {
    centerPanelHeight = centerPanel.clientHeight
  }

  centerStartTotalFlex = fileListFlex.value + diffAreaFlex.value + logAreaFlex.value

  if (target === 'file-diff') {
    centerStartFlex1 = fileListFlex.value
    centerStartFlex2 = diffAreaFlex.value
  } else {
    centerStartFlex1 = diffAreaFlex.value
    centerStartFlex2 = logAreaFlex.value
  }

  document.addEventListener('mousemove', onResizeCenterMove)
  document.addEventListener('mouseup', stopResizeCenter)
  document.body.style.cursor = 'ns-resize'
  document.body.style.userSelect = 'none'
  document.body.style.webkitUserSelect = 'none'
}

function onResizeCenterMove(e: MouseEvent) {
  if (!isResizingCenter.value) return
  const deltaY = e.clientY - centerStartY
  const flexDelta = deltaY * centerStartTotalFlex / centerPanelHeight

  let newFlex1 = centerStartFlex1 + flexDelta
  let newFlex2 = centerStartFlex2 - flexDelta

  if (newFlex1 < CENTER_MIN_FLEX) {
    newFlex1 = CENTER_MIN_FLEX
    newFlex2 = centerStartFlex1 + centerStartFlex2 - CENTER_MIN_FLEX
  }
  if (newFlex2 < CENTER_MIN_FLEX) {
    newFlex2 = CENTER_MIN_FLEX
    newFlex1 = centerStartFlex1 + centerStartFlex2 - CENTER_MIN_FLEX
  }

  if (centerResizeTarget === 'file-diff') {
    fileListFlex.value = newFlex1
    diffAreaFlex.value = newFlex2
  } else {
    diffAreaFlex.value = newFlex1
    logAreaFlex.value = newFlex2
  }
}

function stopResizeCenter() {
  isResizingCenter.value = false
  document.removeEventListener('mousemove', onResizeCenterMove)
  document.removeEventListener('mouseup', stopResizeCenter)
  document.body.style.cursor = ''
  document.body.style.userSelect = ''
  document.body.style.webkitUserSelect = ''
}

// ===== 左侧面板（仓库面板）左右拖拽 =====
let leftResizeStartX = 0
let leftResizeStartWidth = 0

function startResizeLeftPanel(e: MouseEvent) {
  if (e.button !== 0) return
  e.preventDefault()
  isResizingLeftPanel.value = true
  leftResizeStartX = e.clientX
  leftResizeStartWidth = leftPanelWidth.value
  document.addEventListener('mousemove', onResizeLeftPanelMove)
  document.addEventListener('mouseup', stopResizeLeftPanel)
  document.body.style.cursor = 'col-resize'
  document.body.style.userSelect = 'none'
  document.body.style.webkitUserSelect = 'none'
}

function onResizeLeftPanelMove(e: MouseEvent) {
  if (!isResizingLeftPanel.value) return
  const deltaX = e.clientX - leftResizeStartX
  const newWidth = leftResizeStartWidth + deltaX
  leftPanelWidth.value = Math.min(LEFT_PANEL_MAX, Math.max(LEFT_PANEL_MIN, newWidth))
}

function stopResizeLeftPanel() {
  isResizingLeftPanel.value = false
  document.removeEventListener('mousemove', onResizeLeftPanelMove)
  document.removeEventListener('mouseup', stopResizeLeftPanel)
  document.body.style.cursor = ''
  document.body.style.userSelect = ''
  document.body.style.webkitUserSelect = ''
}

// ===== 主界面逻辑 =====

// 切换仓库指示：开始计时，超过 1 秒仍未完成则显示"切换中"图标
function startSwitchIndicator(path: string) {
  switchingPath.value = path
  switchingShown.value = false
  if (switchTimer) clearTimeout(switchTimer)
  switchTimer = setTimeout(() => {
    if (switchingPath.value === path) switchingShown.value = true
  }, 1000)
}

// 切换完成（成功或失败）恢复图标
function stopSwitchIndicator() {
  if (switchTimer) {
    clearTimeout(switchTimer)
    switchTimer = null
  }
  switchingPath.value = null
  switchingShown.value = false
}

async function openScannedProject(project: ScannedProject) {
  startSwitchIndicator(project.path)
  try {
    stopFileWatcher()
    const info = await openRepository(project.path)
    await saveRecentRepository(info)
    repoPath.value = project.path
    selectedFiles.value = []
    selectedFileDiff.value = null
    fileViewMode.value = 'working-tree'
    currentCommitId.value = ''
    commitFiles.value = []
    diffRequestSeq++
    stopDiffLoading()
    localStorage.setItem(LAST_REPO_KEY, project.path)
    await loadRepoData()
  } catch (e) {
    console.error('Open repo error:', e)
  } finally {
    // 切换完成（成功或失败）恢复项目图标
    stopSwitchIndicator()
  }
}

/** 去掉 Windows 长路径前缀 \\?\  */
function sanitizePath(p: string): string {
  return p.replace(/^\\\\\?\\/, '')
}
function sanitizeProjects(projects: ScannedProject[]): ScannedProject[] {
  return projects.map(p => ({ ...p, path: sanitizePath(p.path) }))
}

async function autoScanProjects() {
  isScanning.value = true
  try {
    const projects = sanitizeProjects(await scanProjects())
    scannedProjects.value = projects
    // 保存扫描结果（写入缓存，下次启动直接秒开、不再扫盘）
    localStorage.setItem(SCANNED_PROJECTS_KEY, JSON.stringify(projects))
    console.log('[SnapGit] 仓库扫描完成，已缓存', projects.length, '个仓库')
  } catch (e) {
    // 扫描失败也写入空缓存：避免每次启动都重试扫盘造成反复卡顿。
    // 用户后续可用「重新扫描本地仓库」按钮或手动打开仓库重建列表。
    localStorage.setItem(SCANNED_PROJECTS_KEY, JSON.stringify([]))
    console.error('[SnapGit] 仓库扫描失败，已写入空缓存避免反复扫描:', e)
  } finally {
    isScanning.value = false
  }
}

async function handleOpenRepo() {
  const path = await openFolderDialog()
  if (!path) return
  try {
    stopFileWatcher()
    const info = await openRepository(path)
    await applyRepoInfo(path, info)
  } catch (e) {
    // 目录不是有效 Git 仓库（Rust 端 `open_repository` 错误以 NOT_A_REPO: 为前缀）：
    // 弹"是否初始化为新仓库"对话框，让用户选择 Initialize 或取消。
    const msg = (typeof e === 'string' ? e : e?.toString?.() || String(e))
    if (msg.startsWith('NOT_A_REPO:')) {
      pendingInitPath.value = path
      initing.value = false
      showInitRepoDialog.value = true
    } else {
      console.error('Open error:', e)
    }
  }
}

/**
 * "打开仓库"成功后的统一副作用：
 *  - 写入 recent + scannedProjects 缓存
 *  - 重置面板选择态
 *  - 触发 loadRepoData 加载 git 数据
 * handleOpenRepo 与"打开普通文件夹后 Initialize"两条路径都复用本函数。
 */
async function applyRepoInfo(path: string, info: { name: string; current_branch: string }) {
  await saveRecentRepository({
    path,
    name: info.name,
    current_branch: info.current_branch,
  } as any)
  repoPath.value = path
  selectedFiles.value = []
  selectedFileDiff.value = null
  fileViewMode.value = 'working-tree'
  currentCommitId.value = ''
  commitFiles.value = []
  diffRequestSeq++
  stopDiffLoading()
  const cleanPath = sanitizePath(path)
  if (!scannedProjects.value.some(p => p.path === cleanPath)) {
    scannedProjects.value.unshift({
      path: cleanPath,
      name: info.name || cleanPath.split(/[\\/]/).pop() || cleanPath,
      vcs_type: 'git',
      current_branch: info.current_branch,
    })
    localStorage.setItem(SCANNED_PROJECTS_KEY, JSON.stringify(scannedProjects.value))
  }
  localStorage.setItem(LAST_REPO_KEY, path)
  await loadRepoData()
}

/**
 * 用户在 init 对话框里点了 Initialize：
 * 调 Rust `init_repository` 把目录初始化为新仓库（默认主分支 main），
 * 成功后走 applyRepoInfo 完成加载；失败 toast 报错、对话框保持打开供重试。
 */
async function confirmInitRepo() {
  if (!pendingInitPath.value || initing.value) return
  initing.value = true
  try {
    const info = await initRepository(pendingInitPath.value)
    showInitRepoDialog.value = false
    await applyRepoInfo(pendingInitPath.value, info)
    showToast(t('repository.initSuccess', { name: info.name }), 'success')
  } catch (e) {
    console.error('Init repo error:', e)
    initing.value = false
    showToast(t('repository.initFailed', { error: (typeof e === 'string' ? e : e?.toString?.() || String(e)) }), 'error')
  }
}

/**
 * 「拉取无远程」对话框的「创建远程」按钮：
 * 关闭无远程对话框，打开 AddRemoteDialog。AddRemoteDialog 成功后会触发 `added` 事件，
 * 由 onRemoteAdded 完成 toast + 面板刷新。
 */
function openAddRemoteDialog() {
  showNoRemoteDialog.value = false
  showAddRemoteDialog.value = true
}

/**
 * AddRemoteDialog 添加远程成功后回调：提示用户、刷新面板数据（让 Upstream/Runtime 等面板
 * 拿到新的 remote 信息），但**不自动重新触发拉取**——避免用户刚点完创建就要二次失败体验。
 */
async function onRemoteAdded() {
  showAddRemoteDialog.value = false
  showToast(t('repository.remoteAdded', { name: 'origin' }), 'success')
  await loadUpstreamInfo()
}

async function loadRepoData() {
  if (!repoPath.value) return
  // 切库统一：先清空旧仓库的所有面板数据，避免数据错位/残留。
  // （刚 init 的空仓库理论上不会崩，但作为防御性兜底，所有路径先清后拉）
  commits.value = []
  branches.value = []
  stashes.value = []
  selectedFiles.value = []
  selectedFileDiff.value = null
  commitFiles.value = []
  currentBranch.value = ''
  noMoreCommits.value = false
  loadingMoreCommits.value = false
  fileStatuses.value = []
  try {
    const [commitsData, branchesData, statuses, stashesData] = await Promise.all([
      getCommits(repoPath.value, 50),
      getBranches(repoPath.value, false),
      getFileStatus(repoPath.value, false),
      stashList(repoPath.value).catch(() => []),
    ])
    commits.value = commitsData
    noMoreCommits.value = false
    loadingMoreCommits.value = false
    branches.value = branchesData
    fileStatuses.value = statuses
    stashes.value = stashesData
    currentBranch.value = branchesData.find(b => b.is_current)?.name || ''
    // C2: 后台补算全部分支的 ahead/behind（不阻塞切库首屏），稍后填充分支面板徽章
    getBranches(repoPath.value, true)
      .then((full) => { branches.value = full })
      .catch(() => {})
    // 工作区状态弹窗：补齐上游跟踪分支与远程地址
    await loadUpstreamInfo()
  } catch (e) {
    console.error('Load repo error:', e)
  }
}

// 提交日志滚动到底：分页加载更早的 50 条并追加，条数由 commits.length 驱动自动更新
async function loadMoreCommits() {
  if (!repoPath.value || loadingMoreCommits.value || noMoreCommits.value) return
  loadingMoreCommits.value = true
  try {
    const more = await getCommits(repoPath.value, 50, commits.value.length)
    if (more.length === 0) {
      noMoreCommits.value = true
      return
    }
    // 去重追加（skip 期间若有新提交会产生偏移）
    const existing = new Set(commits.value.map(c => c.id))
    const fresh = more.filter(c => !existing.has(c.id))
    commits.value = [...commits.value, ...fresh]
    if (more.length < 50) noMoreCommits.value = true
  } catch (e) {
    console.error('加载更多提交失败:', e)
  } finally {
    loadingMoreCommits.value = false
  }
}

async function loadStashes() {
  if (!repoPath.value) return
  try {
    stashes.value = await stashList(repoPath.value)
  } catch {
    stashes.value = []
  }
}

// 拉取当前分支的上游跟踪分支名与远程地址，供工具栏状态弹窗展示
async function loadUpstreamInfo() {
  if (!repoPath.value) return
  upstreamBranch.value = null
  remoteUrl.value = null
  try {
    const remotes = await getRemotes(repoPath.value)
    if (remotes.length === 0) return
    let upstreamRemote = remotes[0]
    if (currentBranch.value) {
      try {
        const up = await getUpstream(repoPath.value, currentBranch.value)
        if (up) {
          upstreamBranch.value = `${up.remote}/${up.remote_branch}`
          upstreamRemote = up.remote
        }
      } catch {
        // 分支未设置上游，忽略
      }
    }
    try {
      remoteUrl.value = await getRemoteUrl(repoPath.value, upstreamRemote)
    } catch {
      // 取不到远程地址，忽略
    }
  } catch {
    // 忽略上游信息获取失败，不影响主流程
  }
}

async function loadProjectFileTree(projectPath: string) {
  loadingFileTree.value = projectPath
  try {
    const tree = await getFileTree(projectPath)
    fileTrees.value = { ...fileTrees.value, [projectPath]: tree }
  } catch (e) {
    console.error('Load file tree error:', e)
    fileTrees.value = { ...fileTrees.value, [projectPath]: [] }
  } finally {
    loadingFileTree.value = null
  }
}

async function handleSelectFileFromTree(filePath: string) {
  if (!repoPath.value) return
  selectedFiles.value = [filePath]
  diffAutoScroll.value = true
  const seq = ++diffRequestSeq
  startDiffLoading()
  try {
    const diff = await getFileDiff(repoPath.value, filePath)
    if (seq === diffRequestSeq) {
      selectedFileDiff.value = diff
      stopDiffLoading()
    }
  } catch (e) {
    if (seq === diffRequestSeq) {
      selectedFileDiff.value = null
      stopDiffLoading()
    }
    console.error('Get diff error:', e)
  }
}

async function handleSelectFiles(paths: string[]) {
  if (!repoPath.value) return
  selectedFiles.value = paths
  diffAutoScroll.value = true
  // 显示第一个选中文件的差异
  if (paths.length > 0) {
    const seq = ++diffRequestSeq
    startDiffLoading()
    try {
      const diff = await getFileDiff(repoPath.value, paths[0], fileViewMode.value === 'commit' ? currentCommitId.value : undefined)
      if (seq === diffRequestSeq) {
        selectedFileDiff.value = diff
        stopDiffLoading()
      }
    } catch (e) {
      if (seq === diffRequestSeq) {
        selectedFileDiff.value = null
        stopDiffLoading()
      }
      console.error('Diff error:', e)
    }
  } else {
    diffRequestSeq++
    selectedFileDiff.value = null
    stopDiffLoading()
  }
}

async function handleOpenFile(file: FileStatus) {
  if (!repoPath.value) return
  const mode = file.status === 'conflict' || file.status === 'unmerged' ? 'conflict' : 'diff'
  try {
    // 取当前可见的修改文件列表（working tree 或 commit 视图下都是它），
    // 传入编辑窗口以支持「上一个/下一个文件」导航按钮
    const filesList = displayedFiles.value.map((f) => f.path)
    const currentIndex = filesList.indexOf(file.path)
    await openEditWindow(repoPath.value, file.path, mode, filesList, currentIndex >= 0 ? currentIndex : undefined)
  } catch (e) {
    console.error('Open edit window error:', e)
    alert(t('repository.openEditFailed', { error: (typeof e === 'string' ? e : (e as any)?.toString?.() || String(e)) }))
  }
}

async function handleShowCommitFiles(commitId: string) {
  if (!repoPath.value) return
  // 重复双击同一提交：返回工作树模式
  if (fileViewMode.value === 'commit' && currentCommitId.value === commitId) {
    fileViewMode.value = 'working-tree'
    currentCommitId.value = ''
    commitFiles.value = []
    selectedFiles.value = []
    selectedFileDiff.value = null
    return
  }
  try {
    const files = await getCommitFiles(repoPath.value, commitId)
    commitFiles.value = files
    currentCommitId.value = commitId
    fileViewMode.value = 'commit'
    // 双击进入提交详情：同样自动展示第一个文件的差异，与单点击行为一致
    await selectFirstCommitFile(files)
  } catch (e) {
    console.error('Get commit files error:', e)
  }
}

// 提取文件名（与 FileList.sortedFiles 的排序口径一致），用于确定「列表第一个文件」
function fileNameOf(path: string): string {
  const parts = path.replace(/\\/g, '/').split('/')
  return parts[parts.length - 1] || path
}

// 提交文件按文件名排序，与 FileList 展示顺序一致，
// 确保「第一个文件」= 文件列表顶部那一个（而非 Rust 返回的原始顺序）
function firstCommitFilePath(files: FileStatus[]): string | null {
  if (!files.length) return null
  const sorted = [...files].sort((a, b) =>
    fileNameOf(a.path).toLowerCase().localeCompare(fileNameOf(b.path).toLowerCase())
  )
  return sorted[0].path
}

// 选中提交后自动加载第一个文件的差异（复用 handleSelectFiles 的 diff 链路）
async function selectFirstCommitFile(files: FileStatus[]) {
  const first = firstCommitFilePath(files)
  if (first) {
    await handleSelectFiles([first])
  } else {
    selectedFiles.value = []
    selectedFileDiff.value = null
  }
}

// 提交历史视图：单击提交 → 加载该提交文件并切换右侧详情（复用现有 diff 链路）
// 同时自动选中并展示文件列表第一个文件的差异，点一次提交即可见 diff（验收点）
async function handleSelectCommit(commitId: string) {
  if (!repoPath.value) return
  try {
    const files = await getCommitFiles(repoPath.value, commitId)
    commitFiles.value = files
    currentCommitId.value = commitId
    fileViewMode.value = 'commit'
    await selectFirstCommitFile(files)
  } catch (e) {
    console.error('Get commit files error:', e)
  }
}

// 退出提交历史视图，回到工作区视图并清理提交选择态
function exitLogView() {
  mainView.value = 'worktree'
  fileViewMode.value = 'working-tree'
  currentCommitId.value = ''
  commitFiles.value = []
  selectedFiles.value = []
  selectedFileDiff.value = null
}

async function handleStageFiles(paths: string[]) {
  if (!repoPath.value || paths.length === 0) return
  try {
    const results = await Promise.allSettled(
      paths.map(filePath => stageFile(repoPath.value!, filePath))
    )
    const failed = results.filter(r => r.status === 'rejected')
    if (failed.length > 0) {
      console.warn(`${failed.length} 个文件暂存失败`)
    }
    await loadRepoData()
  } catch (e) {
    console.error('Stage error:', e)
  }
}

async function handleDiscardFiles(paths: string[]) {
  if (!repoPath.value || paths.length === 0) return
  try {
    const results = await Promise.allSettled(
      paths.map(filePath => discardFile(repoPath.value!, filePath))
    )
    const failed = results.filter(r => r.status === 'rejected')
    if (failed.length > 0) {
      const msgs = failed.map(r => (r as PromiseRejectedResult).reason).join('\n')
      alert(t('repository.discardFailed', { error: msgs }))
    }
    await loadRepoData()
  } catch (e) {
    console.error('Discard error:', e)
    alert(t('repository.discardFailed', { error: String(e) }))
  }
}

function requestCommitFiles(paths: string[]) {
  if (paths.length === 0) return
  committingFiles.value = paths
  if (paths.length === 1) {
    commitMessage.value = t('repository.modifiedSingle', { name: paths[0].split(/[\\/]/).pop() })
  } else {
    commitMessage.value = t('repository.modifiedMultiple', { n: paths.length })
  }
  showCommitDialog.value = true
}

function handleCommitFromToolbar() {
  if (!repoPath.value) {
    alert(t('repository.noRepoOpen'))
    return
  }
  // 收集所有有变更的文件
  const modified = fileStatuses.value
    .filter(f => f.status !== 'clean')
    .map(f => f.path)
  if (modified.length === 0) {
    alert(t('repository.noChangesToCommit'))
    return
  }
  committingFiles.value = modified
  commitMessage.value = modified.length === 1
    ? t('repository.modifiedSingle', { name: modified[0].split(/[\\/]/).pop() })
    : t('repository.modifiedMultiple', { n: modified.length })
  showCommitDialog.value = true
}

async function handleCommit() {
  if (!repoPath.value || !commitMessage.value.trim() || isCommitting.value) return
  isCommitting.value = true
  try {
    // 如果指定了文件，先暂存
    if (committingFiles.value.length > 0) {
      for (const f of committingFiles.value) {
        await stageFile(repoPath.value, f)
      }
    }
    await commitChanges(repoPath.value, commitMessage.value.trim())
    commitMessage.value = ''
    showCommitDialog.value = false
    committingFiles.value = []
    await loadRepoData()
    selectedFiles.value = []
    selectedFileDiff.value = null
  } catch (e: any) {
    console.error('Commit error:', e)
    alert(t('repository.commitFailed', { error: (typeof e === 'string' ? e : e?.toString?.() || String(e)) }))
  } finally {
    isCommitting.value = false
  }
}

// 提交并推送：先 commit，再用 upstream 自动推送；无上游或认证失败时退化为打开推送对话框
async function handleCommitAndPush() {
  if (!repoPath.value || !commitMessage.value.trim() || isCommitting.value) return
  isCommitting.value = true
  const committedBranch = currentBranch.value
  try {
    // 1. 提交
    if (committingFiles.value.length > 0) {
      for (const f of committingFiles.value) {
        await stageFile(repoPath.value, f)
      }
    }
    await commitChanges(repoPath.value, commitMessage.value.trim())
    commitMessage.value = ''
    showCommitDialog.value = false
    committingFiles.value = []
    await loadRepoData()
    selectedFiles.value = []
    selectedFileDiff.value = null
  } catch (e: any) {
    console.error('Commit error:', e)
    alert(t('repository.commitFailed', { error: (typeof e === 'string' ? e : e?.toString?.() || String(e)) }))
    return
  } finally {
    isCommitting.value = false
  }

  // 2. 推送（commit 已成功，push 失败不影响提交结果）
  if (!committedBranch) {
    showToast(t('repository.pushSkippedNoBranch'), 'error')
    return
  }
  isPushing.value = true
  try {
    const upstream = await getUpstream(repoPath.value, committedBranch)
    if (!upstream) {
      const remotes = await getRemotes(repoPath.value)
      if (remotes.length === 0) {
        showToast(t('repository.pushSkippedNoRemote'), 'error')
        return
      }
      // 有远程但分支未设置上游：打开推送对话框让用户配置
      pushDialogBranch.value = committedBranch
      showPushDialog.value = true
      showToast(t('repository.pushNeedsUpstream'), 'success')
      return
    }
    await pushChanges(repoPath.value, upstream.remote, committedBranch, upstream.remote_branch)
    // 触发工具栏"已推送"反馈
    pushState.value = 'done'
    if (pushStateTimer) clearTimeout(pushStateTimer)
    pushStateTimer = setTimeout(() => {
      pushState.value = 'idle'
    }, 2000)
    showToast(t('repository.pushSuccess'), 'success')
    loadRepoData()
  } catch (e: any) {
    console.error('Push error:', e)
    const msg = typeof e === 'string' ? e : e?.toString?.() || String(e)
    if (/authentication|access denied|认证失败|terminal prompts disabled/i.test(msg)) {
      // 认证失败：让用户在推送对话框里配置凭证
      pushDialogBranch.value = committedBranch
      showPushDialog.value = true
      showToast(t('repository.pushNeedsAuth'), 'error')
    } else {
      showToast(t('repository.pushFailed', { error: msg }), 'error')
    }
  } finally {
    isPushing.value = false
  }
}

function cancelCommit() {
  showCommitDialog.value = false
  committingFiles.value = []
}

const isCheckingOut = ref(false)

// 切换分支后清理提交视图状态，避免引用旧分支的提交/文件（防止渲染或取数异常）
function resetCommitViewState() {
  currentCommitId.value = ''
  commitFiles.value = []
  selectedFiles.value = []
  selectedFileDiff.value = null
  fileViewMode.value = 'working-tree'
}

async function handleCheckoutBranch(branchName: string) {
  if (!repoPath.value || isCheckingOut.value) return
  isCheckingOut.value = true
  checkoutError.value = null
  checkoutTargetBranch.value = branchName
  try {
    await checkoutBranch(repoPath.value, branchName)
    await loadRepoData()
    resetCommitViewState()
    // 菜单栏入口可能传空分支名（走系统分支管理），仅真实分支切换才提示
    if (branchName) showToast(t('repository.checkoutSuccess', { branch: branchName }), 'success')
  } catch (e: any) {
    console.error('Checkout error:', e)
    checkoutError.value = e?.toString() || t('repository.checkoutFailed')
  } finally {
    isCheckingOut.value = false
  }
}

async function handleCheckoutForce(branchName: string) {
  if (!repoPath.value || isCheckingOut.value) return
  isCheckingOut.value = true
  checkoutError.value = null
  try {
    await checkoutBranch(repoPath.value, branchName, true)
    await loadRepoData()
    resetCommitViewState()
    showToast(t('repository.checkoutForceSuccess', { branch: branchName }), 'success')
  } catch (e: any) {
    console.error('Force checkout error:', e)
    alert(t('repository.checkoutForceFailed', { error: e?.toString() || String(e) }))
  } finally {
    isCheckingOut.value = false
  }
}

async function handleCheckoutFastForward(branchName: string) {
  if (!repoPath.value || isCheckingOut.value) return
  isCheckingOut.value = true
  checkoutError.value = null
  try {
    await checkoutBranch(repoPath.value, branchName)
    isPulling.value = true
    try {
      await pullBranch(repoPath.value)
    } finally {
      isPulling.value = false
    }
    await loadRepoData()
    resetCommitViewState()
    showToast(t('repository.checkoutSuccess', { branch: branchName }), 'success')
  } catch (e: any) {
    console.error('Checkout with merge error:', e)
    alert(t('repository.ffMergeFailed', { error: e?.toString() || String(e) }))
  } finally {
    isCheckingOut.value = false
  }
}

function handleCheckoutCancel() {
  checkoutError.value = null
  checkoutTargetBranch.value = ''
}

// 右键远程分支"检出"：在本地创建跟踪分支并切换
async function handleCheckoutRemote(remoteBranchName: string) {
  if (!repoPath.value) return
  checkoutError.value = null
  checkoutTargetBranch.value = remoteBranchName
  const localName = remoteBranchName.split('/').slice(1).join('/') || remoteBranchName
  try {
    await checkoutRemoteBranch(repoPath.value, remoteBranchName)
    await loadRepoData()
    showToast(t('repository.checkoutRemoteSuccess', { local: localName, remote: remoteBranchName }), 'success')
  } catch (e: any) {
    console.error('Checkout remote error:', e)
    checkoutError.value = e?.toString() || t('repository.checkoutRemoteFailed')
  }
}

async function handlePull() {
  if (!repoPath.value) {
    alert(t('repository.noRepoOpen'))
    return
  }
  if (isPulling.value) return
  isPulling.value = true
  try {
    await pullBranch(repoPath.value)
    await loadRepoData()
    lastPullTime.value = Date.now()
    showToast(t('repository.pullSuccess'), 'success')
  } catch (e: any) {
    console.error('Pull error:', e)
    // 没有配置任何远程（刚 init / 还没添加 origin）：弹引导式对话框，引导用户「创建远程」，
    // 而非直接吐一个底层 `fatal: ...` 错误，按下「创建远程」后可继续。
    const msg = typeof e === 'string' ? e : e?.toString?.() || String(e)
    if (msg.startsWith('NO_REMOTE:')) {
      showNoRemoteDialog.value = true
    } else {
      showToast(t('repository.pullFailed', { error: msg }), 'error')
    }
  } finally {
    isPulling.value = false
  }
}

// ===== 储藏（Stash）=====
const showStashDialog = ref(false)
const stashMessage = ref('')
const stashIncludeUntracked = ref(true)
const stashKeepIndex = ref(false)
const isStashing = ref(false)

function handleStash() {
  if (!repoPath.value) {
    alert(t('repository.noRepoOpen'))
    return
  }
  stashMessage.value = ''
  stashIncludeUntracked.value = true
  stashKeepIndex.value = false
  showStashDialog.value = true
}

async function confirmStash() {
  if (!repoPath.value || isStashing.value) return
  isStashing.value = true
  try {
    await stashCreate(
      repoPath.value,
      stashMessage.value.trim(),
      stashIncludeUntracked.value,
      stashKeepIndex.value,
    )
    showStashDialog.value = false
    stashMessage.value = ''
    showToast(t('repository.stashSuccess'), 'success')
    await loadRepoData()
  } catch (e: any) {
    console.error('Stash error:', e)
    showToast(t('repository.stashFailed', { error: (typeof e === 'string' ? e : e?.toString?.() || String(e)) }), 'error')
  } finally {
    isStashing.value = false
  }
}

async function handleStashApply(stashRef: string, keepIndex: boolean) {
  if (!repoPath.value) return
  try {
    await stashApply(repoPath.value, stashRef, keepIndex)
    showToast(t('repository.stashApplySuccess'), 'success')
    await loadRepoData()
  } catch (e: any) {
    console.error('Stash apply error:', e)
    showToast(t('repository.stashApplyFailed', { error: (typeof e === 'string' ? e : e?.toString?.() || String(e)) }), 'error')
  }
}

async function handleStashDrop(stashRef: string) {
  if (!repoPath.value) return
  if (!confirm(t('repository.dropStashConfirm'))) return
  try {
    await stashDrop(repoPath.value, stashRef)
    showToast(t('repository.stashDropped'), 'success')
    await loadStashes()
    // 删除储藏后刷新文件状态（可能储藏的文件恢复了）
    await loadRepoData()
  } catch (e: any) {
    console.error('Stash drop error:', e)
    showToast(t('repository.stashDropFailed', { error: (typeof e === 'string' ? e : e?.toString?.() || String(e)) }), 'error')
  }
}

// Toast 提示
const toastMessage = ref('')
const toastType = ref<'success' | 'error'>('success')
const toastVisible = ref(false)
let toastTimer: ReturnType<typeof setTimeout> | null = null

function showToast(message: string, type: 'success' | 'error' = 'success') {
  toastMessage.value = message
  toastType.value = type
  toastVisible.value = true
  if (toastTimer) clearTimeout(toastTimer)
  toastTimer = setTimeout(() => {
    toastVisible.value = false
  }, 2500)
}

const showPushDialog = ref(false)
/** 推送对话框预选分支（右键分支菜单传入；空则默认当前分支） */
const pushDialogBranch = ref('')
const showCloneDialog = ref(false)
const showRepoConfig = ref(false)
const showCheckoutBranch = ref(false)
const branchPanelRef = ref<InstanceType<typeof BranchPanel> | null>(null)

// 「不是 Git 仓库 → 询问是否初始化为新仓库」对话框状态
// 用户在菜单栏/工具栏打开普通文件夹时触发，确认后调 init_repository 初始化
const showInitRepoDialog = ref(false)
const pendingInitPath = ref('')
const initing = ref(false)

// 「拉取无远程」对话框：拉取按钮在刚 init 的空仓库上点击时弹出。
// 「创建远程」按钮会进一步打开 AddRemoteDialog 引导用户添加 origin。
const showNoRemoteDialog = ref(false)
const showAddRemoteDialog = ref(false)

// 推送成功反馈：工具栏推送按钮短暂显示"已推送"并隐藏 Tooltip
const pushState = ref<'idle' | 'done'>('idle')
let pushStateTimer: ReturnType<typeof setTimeout> | null = null

// 拉取 / 推送中状态（驱动 ToolBar 按钮加载效果）
const isPulling = ref(false)
const isPushing = ref(false)

function handlePush() {
  if (!repoPath.value) {
    alert(t('repository.noRepoOpen'))
    return
  }
  pushDialogBranch.value = ''
  showPushDialog.value = true
}

// 右键分支菜单：推送指定分支
function handlePushBranch(branchName: string) {
  if (!repoPath.value) return
  pushDialogBranch.value = branchName
  showPushDialog.value = true
}

// 右键分支菜单：创建分支（从当前 HEAD 创建，不切换）
async function handleCreateBranch(branchName: string) {
  if (!repoPath.value) return
  try {
    await createBranch(repoPath.value, branchName)
    await loadRepoData()
    showToast(t('repository.branchCreateSuccess', { branch: branchName }), 'success')
  } catch (e: any) {
    console.error('Create branch error:', e)
    alert(t('repository.branchCreateFailed', { error: (typeof e === 'string' ? e : e?.toString?.() || String(e)) }))
  }
}

// 右键分支菜单：将指定分支合并到当前分支
async function handleMergeBranch(branchName: string) {
  if (!repoPath.value) return
  try {
    await mergeBranch(repoPath.value, branchName)
    await loadRepoData()
    showToast(t('repository.branchMergeSuccess', { branch: branchName, current: currentBranch.value }), 'success')
  } catch (e: any) {
    console.error('Merge branch error:', e)
    alert(t('repository.branchMergeFailed', { error: (typeof e === 'string' ? e : e?.toString?.() || String(e)) }))
  }
}

// 右键分支菜单：重命名分支
async function handleRenameBranch(payload: { oldName: string; newName: string }) {
  if (!repoPath.value) return
  try {
    await renameBranch(repoPath.value, payload.oldName, payload.newName)
    await loadRepoData()
    showToast(t('repository.branchRenameSuccess', { old: payload.oldName, new: payload.newName }), 'success')
  } catch (e: any) {
    console.error('Rename branch error:', e)
    alert(t('repository.branchRenameFailed', { error: (typeof e === 'string' ? e : e?.toString?.() || String(e)) }))
  }
}

// 右键分支菜单：删除分支（含可选的删跟踪引用 / 删远程分支）
async function handleDeleteBranch(payload: { name: string; deleteTracking: boolean; deleteRemote: boolean }) {
  if (!repoPath.value) return
  try {
    await deleteBranch(repoPath.value, payload.name, payload.deleteTracking, payload.deleteRemote)
    await loadRepoData()
    const parts = [payload.name]
    if (payload.deleteTracking) parts.push(t('repository.branchDeleteTracking'))
    if (payload.deleteRemote) parts.push(t('repository.branchDeleteRemote'))
    showToast(t('repository.branchDeleteSuccess', { detail: parts.join(' · ') }), 'success')
  } catch (e: any) {
    console.error('Delete branch error:', e)
    alert(t('repository.branchDeleteFailed', { error: (typeof e === 'string' ? e : e?.toString?.() || String(e)) }))
  }
}

// 右键分支菜单：删除远程分支（从远端移除 + 可选删本地跟踪引用）
async function handleDeleteRemoteBranch(payload: { remote: string; remoteBranch: string; deleteTracking: boolean }) {
  if (!repoPath.value) return
  try {
    await deleteRemoteBranch(repoPath.value, payload.remote, payload.remoteBranch, payload.deleteTracking)
    await loadRepoData()
    const detail = payload.deleteTracking
      ? `${payload.remote}/${payload.remoteBranch} · ${t('repository.branchDeleteTracking')}`
      : `${payload.remote}/${payload.remoteBranch}`
    showToast(t('repository.branchDeleteSuccess', { detail }), 'success')
  } catch (e: any) {
    console.error('Delete remote branch error:', e)
    alert(t('repository.branchDeleteFailed', { error: (typeof e === 'string' ? e : e?.toString?.() || String(e)) }))
  }
}

function handlePushed() {
  pushState.value = 'done'
  if (pushStateTimer) clearTimeout(pushStateTimer)
  pushStateTimer = setTimeout(() => {
    pushState.value = 'idle'
  }, 2000)
  loadRepoData()
  showToast(t('repository.pushSuccess'), 'success')
}

/** 克隆成功回调：自动加入面板列表并打开该仓库 */
async function handleCloned(targetPath: string) {
  const cleanPath = sanitizePath(targetPath)
  // 加入面板列表（避免重复）
  if (!scannedProjects.value.some(p => p.path === cleanPath)) {
    const name = cleanPath.replace(/[/\\]+$/, '').split(/[/\\]/).pop() || cleanPath
    scannedProjects.value.unshift({
      path: cleanPath,
      name,
      vcs_type: 'git',
    })
    localStorage.setItem(SCANNED_PROJECTS_KEY, JSON.stringify(scannedProjects.value))
  }
  // 打开新克隆的仓库
  showToast(t('repository.cloneSuccessOpening'), 'success')
  try {
    await openScannedProject({ path: cleanPath, name: '', vcs_type: 'git' })
  } catch (e) {
    showToast(t('repository.cloneSuccessOpenFailed', { error: String(e) }), 'error')
  }
}

function requestDeleteProject(project: ScannedProject) {
  projectToDelete.value = project
  showDeleteDialog.value = true
}

function confirmDeleteProject() {
  if (!projectToDelete.value) return
  const path = projectToDelete.value.path
  scannedProjects.value = scannedProjects.value.filter(p => p.path !== path)
  localStorage.setItem(SCANNED_PROJECTS_KEY, JSON.stringify(scannedProjects.value))

  if (repoPath.value === path) {
    repoPath.value = ''
    localStorage.removeItem(LAST_REPO_KEY)
  }
  showDeleteDialog.value = false
  projectToDelete.value = null
}

function cancelDeleteProject() {
  showDeleteDialog.value = false
  projectToDelete.value = null
}

async function refreshAll() {
  diffRequestSeq++ // 作废在途的 diff 请求，避免旧结果回填
  await loadRepoData()
  selectedFiles.value = []
  selectedFileDiff.value = null
  fileViewMode.value = 'working-tree'
  currentCommitId.value = ''
  commitFiles.value = []
}

// ===== 工作区文件自动刷新 =====

// 判断路径是否位于 .git 内部（git 自身的写操作会频繁触发事件，必须过滤，防止自刷新死循环）
function isGitInternalPath(p: string): boolean {
  return p.replace(/\\/g, '/').split('/').includes('.git')
}

// 常见噪声目录（依赖/编译产物/缓存/IDE 配置）。
// ⚠️ 只用于「是否触发 fs 刷新」的判断，**不会从变更列表里剔除任何文件**：
// 这些目录的变动通常量极大且无意义，跳过刷新可避免界面风暴与闪烁。
const NOISE_DIR_RE = /(^|\/)(node_modules|dist|target|\.git|__pycache__|\.cache|\.idea|\.vscode|coverage)(\/|$)/
function isNoiseDir(path: string): boolean {
  return NOISE_DIR_RE.test(path.replace(/\\/g, '/').toLowerCase())
}

// 变更文件面板不做任何文件类型过滤：新增 / 删除 / 修改的文件全部原样展示，
// 包括 .tmp/.bak/.orig/~ 等编辑器临时文件和 exe/图片等二进制文件，由用户自行决定是否暂存提交。
// 历史备注：曾按扩展名过滤临时文件以防保存瞬间列表闪动，2026-09-04 古哥明确要求零过滤，已移除。

// C3: 自动刷新防抖窗口由 400ms 放宽到 800ms，降低大仓库后台 git 状态扫描频率
function scheduleAutoRefresh() {
  if (fsRefreshTimer) clearTimeout(fsRefreshTimer)
  fsRefreshTimer = setTimeout(() => {
    fsRefreshTimer = null
    refreshFileStatusesAuto()
  }, 800)
}

// 仅刷新文件状态（不动提交/日志视图），并清理已消失的选中项。
// 单飞 + 合并：编辑器保存时 fs 监听 / 窗口聚焦 / file-edited 事件可能在极短窗口内
// 连续触发多次刷新，并发执行会导致重复的 git status/diff 请求与多轮界面重渲染
//（表现为整片闪跳甚至短暂白屏）。这里保证同一时刻只有一次刷新在跑，
// 期间的新触发只置位 pending，结束后立即补跑一次拿到最终状态。
async function refreshFileStatusesAuto() {
  if (!repoPath.value) return
  if (autoRefreshInFlight) {
    autoRefreshPending = true
    return
  }
  autoRefreshInFlight = true
  try {
    // 不做任何文件类型过滤：git 报什么就显示什么（新增/删除/修改一律展示）
    const statuses = await getFileStatus(repoPath.value, true)

    // 内容比对（与返回顺序无关）：path→status 映射完全一致则不重写，
    // 避免 git status --porcelain 对 rename/conflict/untracked 的分组排序在不同
    // 调用间抖动，导致误判"变了"而整片重写 fileStatuses、引发重渲染与界面闪跳
    const prevStatusMap = new Map(fileStatuses.value.map(f => [f.path, f.status]))
    const unchanged =
      statuses.length === fileStatuses.value.length &&
      statuses.every(s => prevStatusMap.get(s.path) === s.status)

    if (!unchanged) {
      fileStatuses.value = statuses
    }

    // 只要有选中文件就刷新差异：工作区文件保存后即使状态字符串不变（仍是 modified），
    // 内容也已更新，必须刷新 diff 才能反映最新改动。
    // refreshPrimaryDiff 内部用 isSameDiff 判定，内容实质未变则跳过赋值，不重渲染、不闪烁。
    // 注意：此处绝不清理 selectedFiles / selectedFileDiff。
    // 编辑器保存文件时该文件会短暂从 git status 消失（temp 写 + rename / 写入中间态），
    // 若在此清空选中项与差异面板，DiffViewer 会误入「选择文件」空态、整片空白。
    // 选中文件只在用户主动操作 / 切库 / 切分支时清理。
    if (fileViewMode.value === 'working-tree' && selectedFiles.value.length > 0) {
      await refreshPrimaryDiff()
    }
  } catch (e) {
    console.warn('自动刷新文件状态失败:', e)
  } finally {
    autoRefreshInFlight = false
    if (autoRefreshPending) {
      autoRefreshPending = false
      refreshFileStatusesAuto()
    }
  }
}

// 判断两份 diff 是否内容完全一致（防止自动刷新时无谓重写导致 DiffViewer 重渲染/滚动重置）
function isSameDiff(a: unknown, b: unknown): boolean {
  if (a === b) return true
  if (!a || !b) return false
  try {
    // 序列化异常（理论上不会发生，Rust 返回的是纯数据）时保守认为不同，触发更新而非崩溃
    return JSON.stringify(a) === JSON.stringify(b)
  } catch {
    return false
  }
}

// 校验 diff 结构完整性，确保喂给 DiffViewer 的数据可被安全渲染，
// 防止非法数据导致渲染错误（主窗口若无 errorHandler 会整窗白屏）
function isValidDiff(d: unknown): d is FileDiff {
  if (!d || typeof d !== 'object') return false
  const diff = d as FileDiff
  if (!Array.isArray(diff.lines)) return false
  return diff.lines.every(
    (l) => !!l && typeof (l as { line_type?: unknown }).line_type === 'string'
  )
}

// 刷新主选中文件的差异（带竞态保护，防止旧请求覆盖新结果）
async function refreshPrimaryDiff() {
  const path = primarySelectedFile.value
  if (!repoPath.value || !path) return
  diffAutoScroll.value = false // 自动刷新：保持差异面板滚动位置不变
  const seq = ++diffRequestSeq
  try {
    const diff = await getFileDiff(repoPath.value, path)
    if (seq !== diffRequestSeq) return
    // 结构异常（如 Rust 端返回了不符合预期的 diff）直接保留旧 diff，
    // 避免把坏数据喂给 DiffViewer 触发渲染错误、进而因无 errorHandler 而整窗白屏
    if (!isValidDiff(diff)) return
    // 内容未变化则跳过赋值，避免 DiffViewer 无谓重渲染
    if (isSameDiff(selectedFileDiff.value, diff)) return
    selectedFileDiff.value = diff
  } catch (e) {
    if (seq !== diffRequestSeq) return
    // 自动刷新（diffAutoScroll=false）时即使读取失败也保留旧 diff，
    // 避免 DiffViewer 误入「选择文件」空态造成整片空白闪烁；
    // 仅在用户主动切换文件（diffAutoScroll=true）时才允许置空
    if (diffAutoScroll.value === false) return
    if (isSameDiff(selectedFileDiff.value, null)) return
    selectedFileDiff.value = null
  }
}

async function startFileWatcher() {
  stopFileWatcher()
  if (!repoPath.value) return
  try {
    // ⚠️ 关键：必须用 watchImmediate，不要用 watch！
    // plugin-fs 前端包 watch() 默认 delayMs=2000，会走 notify_debouncer_full 分支，
    // 其 RecommendedCache 用 walkdir 递归扫描整个目录树构建缓存（含 node_modules
    // 8709 子目录/503M 文件），切库时阻塞 IPC 线程 ~14 秒。watchImmediate 走普通
    // RecommendedWatcher（内核驱动、O(1) 建立、不预扫描），防抖由前端 800ms 防抖承担。
    const { watchImmediate: fsWatch } = await import('@tauri-apps/plugin-fs')
    const opts = { recursive: true }
    try {
      unwatchFs = await fsWatch(repoPath.value, onFsEvent, opts)
    } catch (recErr) {
      // Win32 上递归 watch 偶发失败，退回非递归监听仓库根目录
      console.warn('递归文件监听失败，退回非递归监听:', recErr)
      unwatchFs = await fsWatch(repoPath.value, onFsEvent, { recursive: true })
    }
  } catch (e) {
    console.warn(
      '文件监听不可用，自动刷新降级为窗口聚焦刷新。若此警告反复出现，请完全退出 SnapGit 并重新运行 npm run tauri dev（Rust 端改动需要重启进程才生效）:',
      e,
    )
  }
}

function onFsEvent(event: unknown) {
  const ev = event as { paths?: string[]; kind?: unknown }
  const paths: string[] = ev?.paths || []
  // 过滤 .git 内部事件（git 自身写操作），防止自我刷新死循环
  if (paths.some(p => isGitInternalPath(p))) return
  // 若事件路径全部来自噪声目录（node_modules 等依赖/编译产物），直接跳过刷新。
  // ⚠️ 只跳过刷新触发，不剔除文件：临时文件仍会出现在变更列表里（仅不主动触发扫描）。
  if (paths.length > 0 && paths.every(p => isNoiseDir(p))) return
  // 过滤纯读取类事件（access/close）：VS Code 等编辑器保存文件时除写入外还会伴随
  // 大量读取/句柄关闭事件，内容并未变化，不应触发刷新与重渲染
  const kind = (ev as { kind?: unknown })?.kind
  if (
    kind &&
    typeof kind === 'object' &&
    !Array.isArray(kind) &&
    Object.keys(kind).length > 0 &&
    Object.keys(kind).every(k => k === 'access')
  ) {
    return
  }
  scheduleAutoRefresh()
}

function stopFileWatcher() {
  if (unwatchFs) {
    try { unwatchFs() } catch { /* ignore */ }
    unwatchFs = null
  }
  if (fsRefreshTimer) {
    clearTimeout(fsRefreshTimer)
    fsRefreshTimer = null
  }
}

function onWindowFocus() {
  // 兜底：窗口重新聚焦时也刷新一次。
  // 覆盖两类场景：1) 文件监听漏报；2) 文件监听不可用（如旧二进制缺 fs 插件）时，
  // 外部编辑器保存后切回本窗口的唯一刷新入口。
  // 注意：刷新内部已有状态比对与 diff 内容比对，数据未变不会重写、不会引起闪烁，
  // 因此这里对已选中文件的场景也照常刷新。
  if (repoPath.value) scheduleAutoRefresh()
}

// ===== 原生菜单栏事件（监听后端发射的 menu-action） =====
let unlistenMenuAction: (() => void) | null = null
// 编辑窗口保存文件后发射 file-edited，主窗口监听并刷新数据
let unlistenFileEdited: (() => void) | null = null

function handleMenuAction(payload: any) {
  // 兼容三种来源：
  //   1) 直接传字符串 action（工具栏 / 前端 MenuBar emit）
  //   2) Tauri 原生菜单事件（Windows 前端也经 menu-action 转发）
  //      Tauri v2 原生菜单的 payload 是 { id: "..." }（不是 action）
  //   3) 嵌套 payload（某些版本的 Tauri 会再包一层）
  const action: string = typeof payload === 'string'
    ? payload
    : (payload?.payload?.action ?? payload?.action ??
       payload?.payload?.id ?? payload?.id ?? '')
  switch (action) {
    case 'open-repo':
      handleOpenRepo()
      break
    case 'clone-repo':
      showCloneDialog.value = true
      break
    case 'commit':
      handleCommitFromToolbar()
      break
    case 'push':
      handlePush()
      break
    case 'pull':
      handlePull()
      break
    case 'refresh':
      refreshAll()
      break
    case 'new-branch':
    case 'branch':
      branchPanelRef.value?.openCreateDialog()
      break
    case 'checkout-branch':
      showCheckoutBranch.value = true
      break
    case 'show-working-tree':
      mainView.value = 'worktree'
      refreshAll()
      break
    case 'show-log':
      mainView.value = 'log'
      break
    case 'repo-config':
      showRepoConfig.value = true
      break
    case 'stash':
      handleStash()
      break
    case 'toggle-theme':
      toggleTheme()
      break
    case 'about':
      alert(t('repository.aboutDialog'))
      break
    default:
      // 未处理的菜单动作
      break
  }
}

async function setupMenuListener() {
  try {
    const { listen } = await import('@tauri-apps/api/event')
    unlistenMenuAction = await listen('menu-action', (event) => {
      handleMenuAction(event)
    })
    // 编辑窗口保存后只需刷新文件状态与差异面板（提交/分支/储藏不会因保存文件而变化，避免全量刷新导致界面闪跳）
    unlistenFileEdited = await listen('file-edited', () => {
      refreshFileStatusesAuto()
    })
  } catch (e) {
    console.warn('菜单事件监听失败：', e)
  }
}

// ===== 生命周期 =====
// 仓库切换时启停文件监听（打开仓库 / 切换到最近仓库都会设置 repoPath）
watch(repoPath, (path) => {
  if (path) {
    startFileWatcher()
  } else {
    stopFileWatcher()
  }
})

onMounted(() => {
  setupMenuListener()
  window.addEventListener('focus', onWindowFocus)

  // 优先：加载保存的扫描结果。缓存键存在即视为「有缓存」，绝不触发扫盘。
  const savedProjects = localStorage.getItem(SCANNED_PROJECTS_KEY)
  const hasCache = savedProjects !== null
  if (hasCache) {
    try { scannedProjects.value = sanitizeProjects(JSON.parse(savedProjects)) } catch {}
    console.log('[SnapGit] 命中仓库缓存', scannedProjects.value.length, '个，跳过扫盘')
  } else {
    console.log('[SnapGit] 无仓库缓存，将后台扫描一次')
  }

  const lastRepo = localStorage.getItem(LAST_REPO_KEY)
  const isFirstLaunch = !localStorage.getItem(FIRST_LAUNCH_KEY)

  if (isFirstLaunch) {
    // 首次启动标记只置位一次，避免后续每次都默认打开首个仓库
    localStorage.setItem(FIRST_LAUNCH_KEY, '1')
  }

  // 有最近仓库：先加载它（不 await，UI 立即渲染，数据回来后响应式填充）
  if (lastRepo) {
    repoPath.value = lastRepo
    loadRepoData()
  }

  // 扫描本地仓库：后台执行，绝不阻塞首屏。
  // 关键：只要 localStorage 中存在扫描缓存（hasCache），就绝不扫描，直接秒开；
  // 仅在「完全无缓存」（首次启动或清过缓存）时才后台扫一次，并默认打开第一个仓库。
  if (!hasCache) {
    autoScanProjects()
      .then(() => {
        if (isFirstLaunch && !localStorage.getItem(LAST_REPO_KEY) && scannedProjects.value.length > 0) {
          return openScannedProject(scannedProjects.value[0])
        }
      })
      .catch((e) => console.error('[SnapGit] 首次默认打开仓库失败:', e))
  }
})

onBeforeUnmount(() => {
  stopResizeBranchPanel()
  stopResizeCenter()
  window.removeEventListener('focus', onWindowFocus)
  stopFileWatcher()
  if (diffLoadingTimer) {
    clearTimeout(diffLoadingTimer)
    diffLoadingTimer = null
  }
  if (switchTimer) {
    clearTimeout(switchTimer)
    switchTimer = null
  }
  if (pushStateTimer) {
    clearTimeout(pushStateTimer)
    pushStateTimer = null
  }
  if (unlistenMenuAction) {
    unlistenMenuAction()
    unlistenMenuAction = null
  }
  if (unlistenFileEdited) {
    unlistenFileEdited()
    unlistenFileEdited = null
  }
})
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
      @repo-config="showRepoConfig = true"
    />

    <!-- 工具栏 -->
    <ToolBar
      :current-branch="currentBranch"
      :repository-path="repoPath"
      :branches="branches"
      :upstream="upstreamBranch"
      :working-tree="workingTreeCounts"
      :remote-url="remoteUrl"
      :repo-name="repoName"
      :last-pull-time="lastPullTime"
      :push-state="pushState"
      :pulling="isPulling"
      :pushing="isPushing"
      @open-repo="handleOpenRepo"
      @commit="handleCommitFromToolbar"
      @push="handlePush"
      @pull="handlePull"
      @refresh="refreshAll"
      @checkout-branch="handleCheckoutBranch"
      @branch="handleMenuAction('new-branch')"
      @merge="handleMenuAction('merge')"
      @stash="handleMenuAction('stash')"
      @show-working-tree="refreshAll"
      @show-log="refreshAll"
    />

    <!-- 主布局 -->
    <div class="main-layout">
      <!-- 左侧面板 -->
      <aside class="left-panel" :style="{ width: leftPanelWidth + 'px' }">
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
      </aside>

      <!-- 仓库面板与变更文件面板之间的左右拖拽分隔条 -->
      <div class="resize-handle-col" @mousedown="startResizeLeftPanel"></div>

      <!-- 中间面板 -->
      <section class="center-panel">
        <!-- 工作区视图（默认三段式：文件列表 / 差异 / 提交日志） -->
        <template v-if="mainView === 'worktree'">
          <!-- 上部：修改的文件列表 -->
          <div class="file-list-area" :style="{ flex: fileListFlex }">
            <FileList 
              :files="displayedFiles"
              :selected-files="selectedFiles"
              :view-mode="fileViewMode"
              @select-files="handleSelectFiles"
              @stage-files="handleStageFiles"
              @commit-files="requestCommitFiles"
              @discard-files="handleDiscardFiles"
              @open-file="handleOpenFile"
              @refresh="refreshAll"
            />
          </div>
          <!-- 拖拉分隔条 -->
          <div class="resize-handle" @mousedown="startResizeCenter($event, 'file-diff')"></div>
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
          <div class="resize-handle" @mousedown="startResizeCenter($event, 'diff-log')"></div>
          <!-- 下部：提交日志 -->
          <div class="log-area" :style="{ flex: logAreaFlex }">
            <LogView 
              :commits="commits"
              :selected-id="currentCommitId"
              :loading-more="loadingMoreCommits"
              :no-more="noMoreCommits"
              @select-commit="handleSelectCommit"
              @show-commit-files="handleShowCommitFiles"
              @load-more="loadMoreCommits"
            />
          </div>
        </template>

        <!-- 提交历史视图：左侧日志 / 右侧提交详情（文件 + 差异） -->
        <template v-else>
          <div class="log-view-layout">
            <div class="log-pane">
              <LogView 
                :commits="commits"
                :selected-id="currentCommitId"
                :loading-more="loadingMoreCommits"
                :no-more="noMoreCommits"
                @select-commit="handleSelectCommit"
                @show-commit-files="handleShowCommitFiles"
                @load-more="loadMoreCommits"
                @exit-log="exitLogView"
              />
            </div>
            <!-- 右侧：提交详情（复用现有组件与 diff 链路） -->
            <div class="commit-detail-area">
              <div class="file-list-area" :style="{ flex: fileListFlex }">
                <FileList 
                  :files="commitFiles"
                  :selected-files="selectedFiles"
                  view-mode="commit"
                  @select-files="handleSelectFiles"
                  @open-file="handleOpenFile"
                  @refresh="refreshAll"
                />
              </div>
              <div class="resize-handle" @mousedown="startResizeCenter($event, 'diff-log')"></div>
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
    </div>

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
        @close="showPushDialog = false"
        @pushed="handlePushed"
        @pushing="(v: boolean) => { isPushing = v }"
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
        :repo-path="repoPath"
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

    <!-- 无仓库时的空状态 -->
    <div v-if="!repoPath && scannedProjects.length === 0 && !isScanning" class="empty-state-overlay">
      <div class="empty-state-content">
        <FolderOpen :size="72" class="empty-icon" />
        <h2>{{ t('repository.emptyTitle') }}</h2>
        <p class="empty-sub">{{ t('repository.emptySubtitle') }}</p>
        <div class="empty-actions">
          <button class="btn btn-primary" @click="handleOpenRepo">{{ t('repository.openRepoBtn') }}</button>
          <button class="btn btn-secondary" @click="autoScanProjects">{{ t('repository.rescanBtn') }}</button>
        </div>
      </div>
    </div>

    <!-- Toast 提示 -->
    <Teleport to="body">
      <Transition name="toast">
        <div v-if="toastVisible" class="toast" :class="toastType">
          <span class="toast-icon">{{ toastType === 'success' ? '✓' : '✗' }}</span>
          <span class="toast-msg">{{ toastMessage }}</span>
        </div>
      </Transition>
    </Teleport>
  </div>
</template>

<style scoped>
.app-container {
  display: flex;
  flex-direction: column;
  width: 100%;
  height: 100vh;
  background-color: var(--bg-primary);
  color: var(--text-primary);
  overflow: hidden;
}

/* 主布局 */
.main-layout {
  display: flex;
  flex: 1;
  min-height: 0;
  gap: 0;
}

/* 左侧面板 */
.left-panel {
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  background-color: var(--bg-secondary);
  min-height: 0;
}

.left-panel > :deep(.repository-list) {
  flex: 1;
  min-height: 0;
  overflow: hidden;
  display: flex;
  flex-direction: column;
}

.left-panel > :deep(.branch-panel) {
  min-height: 0;
  overflow: hidden;
  display: flex;
  flex-direction: column;
}

/* 中间面板 */
.center-panel {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-width: 0;
  background-color: var(--bg-secondary);
  min-height: 0;
}

.file-list-area {
  min-height: 0;
  overflow: hidden;
}

.file-list-area > :deep(.file-list) {
  border-right: none;
}

.resize-handle {
  flex-shrink: 0;
  height: 1px;
  padding: 3px 0;
  box-sizing: content-box;
  background-clip: content-box;
  background-color: var(--border-color);
  cursor: ns-resize;
  margin: -3px 0;
  position: relative;
  z-index: 1;
}

/* 仓库面板 ↔ 中间面板的左右拖拽分隔条 */
.resize-handle-col {
  flex-shrink: 0;
  width: 1px;
  padding: 0 3px;
  box-sizing: content-box;
  background-clip: content-box;
  background-color: var(--border-color);
  cursor: col-resize;
  margin: 0 -3px;
  position: relative;
  z-index: 1;
}

.resize-handle-col:hover {
  background-color: var(--accent-primary);
}

.diff-area {
  min-height: 0;
}

.log-area {
  min-height: 0;
}

/* 提交历史视图：左右布局 */
.log-view-layout {
  display: flex;
  flex-direction: row;
  flex: 1;
  min-width: 0;
  min-height: 0;
}

.log-pane {
  flex: 4;
  min-width: 0;
  min-height: 0;
  display: flex;
}

.log-pane > :deep(.log-viewer) {
  border-right: 1px solid var(--border-color);
}

.commit-detail-area {
  flex: 6;
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

/* 空状态遮罩 */
.empty-state-overlay {
  position: fixed;
  inset: 68px 0 0 0;
  display: flex;
  align-items: center;
  justify-content: center;
  pointer-events: none;
  z-index: 50;
}

.empty-state-content {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
  padding: 40px;
  pointer-events: auto;
}

/* 空状态图标：lucide 线性图标，颜色走主题变量（不再依赖 emoji 的 font-size） */
.empty-icon {
  color: var(--text-muted);
  opacity: 0.35;
  line-height: 1;
}

/* "是否初始化为新仓库"对话框：左侧绿色问号圆圈 + 右侧文本（仿 TortoiseHg 风格）。
   颜色走主题变量：深色主题用深绿 + 半透明白边；浅色主题用浅绿 + 深边。 */
.init-repo-row {
  display: flex;
  align-items: flex-start;
  gap: 14px;
}

.init-repo-icon {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 44px;
  height: 44px;
  border-radius: 50%;
  /* 浅色主题：柔和绿；深色主题：更亮的草绿 */
  background: radial-gradient(circle at 30% 30%, #4ade80, #16a34a);
  color: #ffffff;
  box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.2);
}

:global([data-theme='dark']) .init-repo-icon {
  background: radial-gradient(circle at 30% 30%, #22c55e, #15803d);
  box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.15);
}

.init-repo-text {
  flex: 1;
  min-width: 0;
}

.init-repo-desc {
  margin: 6px 0 0;
  font-size: 13px;
  line-height: 1.5;
  color: var(--text-secondary);
}

.empty-state-content h2 {
  font-size: 20px;
  font-weight: 600;
  color: var(--text-primary);
  margin: 16px 0 0 0;
}

.empty-sub {
  font-size: 13px;
  color: var(--text-muted);
  margin: 0;
}

.empty-actions {
  display: flex;
  gap: 12px;
  margin-top: 16px;
}

/* Toast 提示 */
.toast {
  position: fixed;
  top: 50%;
  left: 50%;
  transform: translate(-50%, -50%);
  z-index: 10000;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 20px;
  border-radius: 6px;
  font-size: 13px;
  font-weight: 500;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.2);
  pointer-events: none;
}

.toast.success {
  background-color: var(--bg-secondary);
  color: var(--brand-primary);
  border: 1px solid var(--border-medium);
  border-left: 3px solid var(--brand-primary);
  box-shadow:
    0 10px 30px -8px rgba(15, 23, 42, 0.2),
    0 2px 6px rgba(15, 23, 42, 0.08);
}

.toast.error {
  background-color: #c53030;
  color: #fff;
}

.toast-icon {
  font-size: 15px;
  font-weight: 700;
}

.toast-msg {
  white-space: nowrap;
}

.toast-enter-active {
  transition: all 0.3s ease-out;
}

.toast-leave-active {
  transition: all 0.25s ease-in;
}

.toast-enter-from {
  opacity: 0;
  transform: translate(-50%, calc(-50% - 12px));
}

.toast-leave-to {
  opacity: 0;
  transform: translate(-50%, calc(-50% + 8px));
}

/* ===== Dialog 通用样式（储藏对话框复用） ===== */
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
  flex-direction: column;
  gap: 12px;
  padding: 16px;
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

.btn-cancel {
  background: none;
  color: var(--text-secondary);
  border-color: var(--border-color);
}

.btn-cancel:hover:not(:disabled) {
  background-color: var(--bg-hover);
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

/* ===== 储藏对话框 ===== */
.stash-desc {
  margin-bottom: 12px;
}

.stash-desc p {
  margin: 0 0 4px;
  font-size: 13px;
  color: var(--text-primary);
}

.stash-hint {
  font-size: 12px !important;
  color: var(--text-secondary) !important;
  line-height: 1.5;
}

.stash-label {
  display: block;
  margin-bottom: 6px;
  font-size: 13px;
  color: var(--text-primary);
  font-weight: 500;
}

.textarea-field {
  width: 100%;
  padding: 8px 10px;
  border: 1px solid var(--border-color);
  border-radius: 6px;
  background-color: var(--bg-primary);
  color: var(--text-primary);
  font-size: 13px;
  font-family: inherit;
  resize: vertical;
  outline: none;
  transition: border-color 0.15s;
}

.textarea-field:focus {
  border-color: var(--brand-primary);
}

.stash-options {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin-top: 12px;
  padding-top: 12px;
  border-top: 1px solid var(--border-color);
}

.stash-option {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  color: var(--text-primary);
  cursor: pointer;
}

.stash-option input[type="checkbox"] {
  width: 14px;
  height: 14px;
  accent-color: var(--brand-primary);
  cursor: pointer;
}
</style>