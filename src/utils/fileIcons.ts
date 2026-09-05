import type { Component } from 'vue'
import {
  FileCode, FileText, FileImage, FileVideo, FileAudio, FileArchive,
  FileJson, FileType, FileCog, File, GitFork,
} from 'lucide-vue-next'

/**
 * 按文件扩展名返回对应的 lucide 线性图标组件。
 *
 * ⚠️ 全项目唯一的「文件类型 → 图标」映射：变更文件面板（FileList.vue）
 * 与仓库面板文件树（RepositoryList.vue）共用此函数，保证两处图标风格一致。
 * 新增文件类型时只改这里，不要在各组件里各写一份。
 *
 * 约定：图标统一使用 lucide-vue-next 线性图标，禁止 emoji（跨平台渲染不一致、
 * 彩色破坏专业调性）。图标颜色由调用方用 CSS 变量控制，跟随 currentColor 适配双主题。
 */
export function getFileIcon(filename: string): Component {
  const ext = filename.split('.').pop()?.toLowerCase() || ''
  if (['js', 'ts', 'jsx', 'tsx', 'py', 'java', 'kt', 'groovy', 'cpp', 'c', 'h', 'hpp', 'rs', 'go', 'vue', 'html', 'htm'].includes(ext)) return FileCode
  if (['css', 'scss', 'less', 'sass'].includes(ext)) return FileType
  if (['json', 'yml', 'yaml', 'toml', 'ini'].includes(ext)) return FileJson
  if (['md', 'txt', 'rst', 'pdf'].includes(ext)) return FileText
  if (['png', 'jpg', 'jpeg', 'gif', 'svg', 'webp', 'ico'].includes(ext)) return FileImage
  if (['mp4', 'avi', 'mov', 'wmv'].includes(ext)) return FileVideo
  if (['mp3', 'wav', 'flac', 'ogg'].includes(ext)) return FileAudio
  if (['zip', 'rar', '7z', 'tar', 'gz'].includes(ext)) return FileArchive
  if (['exe', 'msi', 'bat', 'sh', 'cmd', 'ps1'].includes(ext)) return FileCog
  if (['gitignore', 'gitattributes', 'gitmodules'].includes(filename.toLowerCase()) || filename.startsWith('.git')) return GitFork
  return File
}
