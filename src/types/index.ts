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
