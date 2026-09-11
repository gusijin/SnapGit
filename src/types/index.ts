export interface Commit {
  id: string
  message: string
  author: string
  date: string
}

export interface Branch {
  name: string
  is_current: boolean
  is_remote: boolean
  ahead: number
  behind: number
}

export interface FileStatus {
  path: string
  status: string
}

export interface RepositoryInfo {
  path: string
  name: string
  current_branch: string
}

export interface ScannedProject {
  path: string
  name: string
  vcs_type: string
  current_branch?: string
}

export interface FileTreeNode {
  name: string
  path: string
  is_dir: boolean
  children: FileTreeNode[]
}

export interface DiffSegment {
  text: string
  changed: boolean
}

export interface DiffLine {
  line_type: string
  content: string
  new_content: string
  old_line: number | null
  new_line: number | null
  old_segments: DiffSegment[]
  new_segments: DiffSegment[]
}

export interface FileDiff {
  old_content: string[]
  new_content: string[]
  lines: DiffLine[]
  /** 二进制文件（exe / 图片 / 压缩包等）：无文本 diff，前端展示「无法预览」提示 */
  is_binary: boolean
  /** 超大文本文件：行数超过阈值，Rust 端跳过了 LCS 计算，前端降级为「仅展示可编辑、无差异高亮」 */
  is_oversized: boolean
  /** 文件「实际」行尾符：工作区模式=工作区文件，提交模式=当前提交版本。取值 "CRLF" / "LF" / null */
  eol_actual?: string | null
  /** 文件「期望」行尾符：工作区模式=git 按 autocrlf/.gitattributes 期望的形态，提交模式=父版本。
   *  `eol_actual !== eol_expected` 即「内容字符一致、但 git 认为有差异」的根因（行尾规范化）。 */
  eol_expected?: string | null
}

export interface ConflictBlock {
  start_line: number
  separator_line: number
  end_line: number
  ours: string[]
  theirs: string[]
  base?: string[]
  marker_ours: string
  marker_theirs: string
}

export interface ConflictFile {
  path: string
  ours_content: string[]
  theirs_content: string[]
  base_content?: string[]
  working_content: string[]
  blocks: ConflictBlock[]
}

export interface StashEntry {
  index: number
  branch: string
  message: string
  date: string
  stash_ref: string
}

export interface SubmoduleInfo {
  /** 子模块在父仓库中的相对路径 */
  path: string
  /** 展示名（path 最后一段） */
  name: string
  /** 远程地址（来自 .gitmodules） */
  url: string
  /** 跟踪分支（可空） */
  branch?: string | null
  /** 当前已检出的提交完整 SHA；未初始化为 null */
  head_commit?: string | null
  /** 当前已检出的提交短 SHA（前 7 位）；未初始化为 null */
  head_commit_short?: string | null
  /** 父仓库记录的 gitlink 提交 SHA；未记录为 null */
  recorded_commit?: string | null
  /** 是否已初始化 */
  initialized: boolean
  /** 指针是否改变：当前 HEAD ≠ 记录值 */
  modified: boolean
  /** 子模块工作树是否有未提交改动 */
  dirty: boolean
}
