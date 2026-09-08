import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-shell'

export interface GitInfo {
  available: boolean
  version: string
  path: string
  os: string
}

// 全局 Git 可用性状态：启动时检测一次，无 Git 时弹出引导安装弹窗。
const gitAvailable = ref(true)
const gitVersion = ref('')
const gitPath = ref('')
const gitOs = ref('')
const showGitRequired = ref(false)
const detecting = ref(false)

async function detectGit() {
  detecting.value = true
  try {
    const info = await invoke<GitInfo>('detect_git')
    gitAvailable.value = info.available
    gitVersion.value = info.version
    gitPath.value = info.path
    gitOs.value = info.os
    if (!info.available) showGitRequired.value = true
  } catch {
    // 命令异常也当作不可用，弹引导（避免静默失败）
    gitAvailable.value = false
    showGitRequired.value = true
  } finally {
    detecting.value = false
  }
}

// 按平台打开 Git 下载页（macOS 同时提示 xcode-select --install）
function openGitDownload() {
  const target =
    gitOs.value === 'macos'
      ? 'https://git-scm.com/download/mac'
      : gitOs.value === 'linux'
        ? 'https://git-scm.com/download/linux'
        : 'https://git-scm.com/download/win'
  open(target).catch(() => {})
}

function retryDetect() {
  showGitRequired.value = false
  detectGit()
}

export function useGit() {
  return {
    gitAvailable,
    gitVersion,
    gitPath,
    gitOs,
    showGitRequired,
    detecting,
    detectGit,
    openGitDownload,
    retryDetect,
  }
}
