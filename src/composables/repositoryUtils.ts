import type { FileDiff, FileStatus, ScannedProject } from '../types'

/** Remove the Windows long path prefix before persisting/displaying paths. */
export function sanitizePath(path: string): string {
  return path.replace(/^\\\\\?\\/, '')
}

export function sanitizeProjects(projects: ScannedProject[]): ScannedProject[] {
  return projects.map(project => ({ ...project, path: sanitizePath(project.path) }))
}

export function fileNameOf(path: string): string {
  const parts = path.replace(/\\/g, '/').split('/')
  return parts[parts.length - 1] || path
}

export function firstCommitFilePath(files: FileStatus[]): string | null {
  if (!files.length) return null
  const sorted = [...files].sort((a, b) =>
    fileNameOf(a.path).toLowerCase().localeCompare(fileNameOf(b.path).toLowerCase())
  )
  return sorted[0].path
}

export function isNothingToCommit(message: string): boolean {
  return message.includes('nothing to commit') || message.includes('no changes added to commit')
}

export const NOISE_DIR_RE = /(^|\/)(node_modules|dist|target|\.git|__pycache__|\.cache|\.idea|\.vscode|coverage)(\/|$)/

export function isGitInternalPath(path: string): boolean {
  return path.replace(/\\/g, '/').split('/').includes('.git')
}

export function isNoiseDir(path: string): boolean {
  return NOISE_DIR_RE.test(path.replace(/\\/g, '/').toLowerCase())
}

export function isSameDiff(a: unknown, b: unknown): boolean {
  if (a === b) return true
  if (!a || !b) return false
  try {
    return JSON.stringify(a) === JSON.stringify(b)
  } catch {
    return false
  }
}

export function isValidDiff(value: unknown): value is FileDiff {
  if (!value || typeof value !== 'object') return false
  const diff = value as FileDiff
  if (!Array.isArray(diff.lines)) return false
  return diff.lines.every(line => !!line && typeof (line as { line_type?: unknown }).line_type === 'string')
}
