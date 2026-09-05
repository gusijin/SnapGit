import { invoke } from '@tauri-apps/api/core'
import type { Commit, Branch, FileStatus, RepositoryInfo, ScannedProject, FileTreeNode, FileDiff, ConflictFile, StashEntry } from '../types'

export async function openRepository(path: string): Promise<RepositoryInfo> {
  return await invoke('open_repository', { path })
}

/**
 * 把指定目录初始化为新 Git 仓库（默认主分支 main）。
 * 用于"打开普通文件夹"流程：openRepository 失败 + 错误为 NOT_A_REPO 前缀时调用。
 */
export async function initRepository(path: string): Promise<RepositoryInfo> {
  return await invoke('init_repository', { repoPath: path })
}

export async function loadRecentRepositories(): Promise<RepositoryInfo[]> {
  return await invoke('load_recent_repositories')
}

export async function saveRecentRepository(repoInfo: RepositoryInfo): Promise<void> {
  await invoke('save_recent_repository', { repoInfo })
}

export async function removeRecentRepository(path: string): Promise<void> {
  await invoke('remove_recent_repository', { path })
}

export async function getCommits(repoPath: string, limit: number = 50, skip: number = 0): Promise<Commit[]> {
  return await invoke('get_commits', { repoPath, limit, skip })
}

export async function getBranches(repoPath: string, full: boolean = false): Promise<Branch[]> {
  return await invoke('get_branches', { repoPath, full })
}

export async function getRemotes(repoPath: string): Promise<string[]> {
  return await invoke('get_remotes', { repoPath })
}

/// 添加远程仓库（git remote add <name> <url>）。
/// 用于「拉取无远程」对话框的「创建远程」按钮。
/// - `name`：远程简称（如 `origin`）
/// - `url`：远程仓库 URL（https/ssh/git 协议均可）
export async function addRemote(repoPath: string, name: string, url: string): Promise<void> {
  await invoke('add_remote', { repoPath, name, url })
}

export async function getUpstream(repoPath: string, branchName: string): Promise<{ remote: string; remote_branch: string } | null> {
  return await invoke('get_upstream', { repoPath, branchName })
}

export async function getFileStatus(repoPath: string, cached: boolean = true): Promise<FileStatus[]> {
  return await invoke('get_file_status', { repoPath, cached })
}

export async function stageFile(repoPath: string, filePath: string): Promise<void> {
  await invoke('stage_file', { repoPath, filePath })
}

export async function discardFile(repoPath: string, filePath: string): Promise<void> {
  await invoke('discard_file', { repoPath, filePath })
}

export async function commitChanges(repoPath: string, message: string): Promise<string> {
  return await invoke('commit', { repoPath, message })
}

export async function pushChanges(repoPath: string, remote: string, localBranch: string, remoteBranch: string): Promise<void> {
  await invoke('push', { repoPath, remote, localBranch, remoteBranch })
}

export async function getRemoteUrl(repoPath: string, remoteName: string): Promise<string> {
  return await invoke('get_remote_url', { repoPath, remoteName })
}

export async function saveCredentials(repoPath: string, remoteName: string, username: string, token: string): Promise<void> {
  await invoke('save_credentials', { repoPath, remoteName, username, token })
}

export async function checkoutBranch(repoPath: string, branchName: string, force?: boolean): Promise<void> {
  await invoke('checkout_branch', { repoPath, branchName, force: force ?? false })
}

// 从远程分支检出：在本地创建跟踪分支并切换（如本地已存在同名分支则直接切换）
export async function checkoutRemoteBranch(repoPath: string, remoteBranch: string): Promise<void> {
  await invoke('checkout_remote_branch', { repoPath, remoteBranch })
}

export async function pullBranch(repoPath: string): Promise<void> {
  await invoke('pull_branch', { repoPath })
}

export async function createBranch(repoPath: string, branchName: string): Promise<void> {
  await invoke('create_branch', { repoPath, branchName })
}

export async function getCurrentBranch(repoPath: string): Promise<string> {
  return await invoke('get_current_branch', { repoPath })
}

export async function mergeBranch(repoPath: string, branchName: string): Promise<void> {
  await invoke('merge_branch', { repoPath, branchName })
}

export async function renameBranch(repoPath: string, oldName: string, newName: string): Promise<void> {
  await invoke('rename_branch', { repoPath, oldName, newName })
}

export async function deleteBranch(
  repoPath: string,
  branchName: string,
  deleteTracking: boolean,
  deleteRemote: boolean,
): Promise<void> {
  await invoke('delete_branch', { repoPath, branchName, deleteTracking, deleteRemote })
}

export async function deleteRemoteBranch(
  repoPath: string,
  remote: string,
  remoteBranch: string,
  deleteTracking: boolean,
): Promise<void> {
  await invoke('delete_remote_branch', { repoPath, remote, remoteBranch, deleteTracking })
}

export interface RemoteDeleteCheck {
  can_delete: boolean
  reason: string | null
}

/**
 * 检测远程分支是否可被当前用户删除（dry-run + 协议快速检查）。
 * 用于删除弹窗打开时让"从远程删除"复选框在不可删时变灰、不可勾选。
 */
export async function checkRemoteBranchDeletable(
  repoPath: string,
  remote: string,
  remoteBranch: string,
): Promise<RemoteDeleteCheck> {
  return await invoke<RemoteDeleteCheck>('check_remote_branch_deletable', {
    repoPath,
    remote,
    remoteBranch,
  })
}

export async function openFolderDialog(): Promise<string | null> {
  const result = await invoke<string | null>('open_folder_dialog')
  return result
}

export async function scanProjects(): Promise<ScannedProject[]> {
  return await invoke('scan_projects')
}

export async function getFileTree(repoPath: string): Promise<FileTreeNode[]> {
  return await invoke('get_file_tree', { repoPath })
}

export async function getFileDiff(repoPath: string, filePath: string, commitId?: string): Promise<FileDiff> {
  return await invoke('get_file_diff', { repoPath, filePath, commitId: commitId ?? null })
}

export async function getCommitFiles(repoPath: string, commitId: string): Promise<FileStatus[]> {
  return await invoke('get_commit_files', { repoPath, commitId })
}

export async function getConflictFile(repoPath: string, filePath: string): Promise<ConflictFile> {
  return await invoke('get_conflict_file', { repoPath, filePath })
}

export async function readWorkingFile(repoPath: string, filePath: string): Promise<string> {
  return await invoke('read_working_file', { repoPath, filePath })
}

export async function writeFileContent(repoPath: string, filePath: string, content: string): Promise<void> {
  await invoke('write_file_content', { repoPath, filePath, content })
}

// 在独立新窗口中打开文件编辑/冲突解决器。
// filesList + currentIndex 让编辑窗口在多个修改文件间支持「上一个 / 下一个」跳转，
// 单文件场景可省略（按钮自动 disabled）。
export async function openEditWindow(
  repoPath: string,
  filePath: string,
  mode: 'conflict' | 'diff',
  filesList?: string[],
  currentIndex?: number,
): Promise<void> {
  await invoke('open_edit_window', {
    repoPath,
    filePath,
    mode,
    filesList: filesList ?? null,
    currentIndex: currentIndex ?? null,
  })
}

// 关闭编辑窗口（会同时通知主窗口刷新数据）
export async function closeEditWindow(): Promise<void> {
  await invoke('close_edit_window')
}

// 独立编辑窗口启动后，主动从 Rust 端取回打开时传入的参数。
// 比 initialization_script 更稳，能避开 dev 模式下 script 注入失效的问题。
// 传入显式 label 可绕过 Tauri Window 参数自动解析的潜在问题。
export async function getEditArgs(label?: string): Promise<{ repoPath: string; filePath: string; mode: 'conflict' | 'diff' }> {
  return await invoke('get_edit_args', { label: label ?? null })
}

// ===== 储藏（Stash）=====

export async function stashCreate(
  repoPath: string,
  message: string,
  includeUntracked: boolean = true,
  keepIndex: boolean = false,
): Promise<void> {
  await invoke('stash_create', {
    repoPath,
    message,
    includeUntracked,
    keepIndex,
  })
}

export async function stashList(repoPath: string): Promise<StashEntry[]> {
  return await invoke('stash_list', { repoPath })
}

export async function stashApply(repoPath: string, stashRef: string, keepIndex: boolean = false): Promise<void> {
  await invoke('stash_apply', { repoPath, stashRef, keepIndex })
}

export async function stashDrop(repoPath: string, stashRef: string): Promise<void> {
  await invoke('stash_drop', { repoPath, stashRef })
}

// ===== 克隆仓库 =====

export interface CloneRepositoryOptions {
  url: string
  /** 最终目标目录（不存在则 git 自动创建；已存在则必须为空） */
  targetDir: string
  /** 指定检出分支；留空使用远程默认分支 */
  branch?: string
  /** 同时初始化子模块（--recurse-submodules） */
  includeSubmodules?: boolean
  /** 同时获取所有标签（--tags） */
  fetchTags?: boolean
}

export async function cloneRepository(opts: CloneRepositoryOptions): Promise<string> {
  return await invoke('clone_repository', {
    url: opts.url,
    targetDir: opts.targetDir,
    branch: opts.branch ?? '',
    includeSubmodules: opts.includeSubmodules ?? false,
    fetchTags: opts.fetchTags ?? false,
  })
}

// ===== 读取仓库配置（远程 / SSH / 凭证 等） =====

export interface RemoteConfig {
  name: string
  fetch_url: string
  push_url: string
}

export interface SshKeyInfo {
  path: string
  exists: boolean
}

export interface RepoConfig {
  remotes: RemoteConfig[]
  user_name: string | null
  user_email: string | null
  credential_helper: string | null
  core_ssh_command: string | null
  ssh_keys: SshKeyInfo[]
  git_ssh_command_env: string | null
  ssh_auth_sock_env: string | null
  local_config: Record<string, string>
}

export async function getRepoConfig(repoPath: string): Promise<RepoConfig> {
  return await invoke('get_repo_config', { repoPath })
}