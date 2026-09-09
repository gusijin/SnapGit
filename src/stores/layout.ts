import { ref, watch } from 'vue'

/**
 * 主界面面板布局尺寸（拖拽后持久化，下次启动自动恢复）
 * - leftPanelWidth: 左侧仓库面板宽度（px，左右拖拽）
 * - branchPanelFlex: 左栏「分支面板」相对「仓库列表」的 flex（上下拖拽）
 * - fileListFlex / diffAreaFlex / logAreaFlex: 中栏三块区域的 flex（上下拖拽）
 */
export interface LayoutState {
  leftPanelWidth: number
  branchPanelFlex: number
  fileListFlex: number
  diffAreaFlex: number
  logAreaFlex: number
}

const STORAGE_KEY = 'snapgit-layout'

// ===== 边界（单一来源，拖拽逻辑与持久化校验共用） =====
export const LEFT_PANEL_MIN = 200
export const LEFT_PANEL_MAX = 560
export const BRANCH_PANEL_MIN_FLEX = 0.5
export const BRANCH_PANEL_MAX_FLEX = 5.0
export const CENTER_MIN_FLEX = 0.2
const CENTER_MAX_FLEX = 20

const DEFAULTS: LayoutState = {
  leftPanelWidth: 320,
  branchPanelFlex: 1,
  fileListFlex: 1,
  diffAreaFlex: 1.4,
  logAreaFlex: 1,
}

function clampNum(v: unknown, min: number, max: number, fallback: number): number {
  if (typeof v !== 'number' || !Number.isFinite(v)) return fallback
  return Math.min(max, Math.max(min, v))
}

function loadLayout(): LayoutState {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return { ...DEFAULTS }
    const p = JSON.parse(raw) as Partial<LayoutState>
    return {
      leftPanelWidth: clampNum(p.leftPanelWidth, LEFT_PANEL_MIN, LEFT_PANEL_MAX, DEFAULTS.leftPanelWidth),
      branchPanelFlex: clampNum(p.branchPanelFlex, BRANCH_PANEL_MIN_FLEX, BRANCH_PANEL_MAX_FLEX, DEFAULTS.branchPanelFlex),
      fileListFlex: clampNum(p.fileListFlex, CENTER_MIN_FLEX, CENTER_MAX_FLEX, DEFAULTS.fileListFlex),
      diffAreaFlex: clampNum(p.diffAreaFlex, CENTER_MIN_FLEX, CENTER_MAX_FLEX, DEFAULTS.diffAreaFlex),
      logAreaFlex: clampNum(p.logAreaFlex, CENTER_MIN_FLEX, CENTER_MAX_FLEX, DEFAULTS.logAreaFlex),
    }
  } catch {
    return { ...DEFAULTS }
  }
}

const saved = loadLayout()

const leftPanelWidth = ref(saved.leftPanelWidth)
const branchPanelFlex = ref(saved.branchPanelFlex)
const fileListFlex = ref(saved.fileListFlex)
const diffAreaFlex = ref(saved.diffAreaFlex)
const logAreaFlex = ref(saved.logAreaFlex)

let saveTimer: ReturnType<typeof setTimeout> | null = null

function persist() {
  const state: LayoutState = {
    leftPanelWidth: leftPanelWidth.value,
    branchPanelFlex: branchPanelFlex.value,
    fileListFlex: fileListFlex.value,
    diffAreaFlex: diffAreaFlex.value,
    logAreaFlex: logAreaFlex.value,
  }
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(state))
  } catch {
    // localStorage 不可用（隐私模式等）时静默降级：布局仍可用，只是不持久化
  }
}

/** 立即落盘（拖拽结束时调用，避免 debounce 窗口内退出导致丢失） */
export function flushLayout() {
  if (saveTimer) {
    clearTimeout(saveTimer)
    saveTimer = null
  }
  persist()
}

function scheduleSave() {
  if (saveTimer) clearTimeout(saveTimer)
  saveTimer = setTimeout(() => {
    saveTimer = null
    persist()
  }, 250)
}

watch([leftPanelWidth, branchPanelFlex, fileListFlex, diffAreaFlex, logAreaFlex], scheduleSave)

// 窗口关闭/刷新前兜底落盘
window.addEventListener('beforeunload', flushLayout)

/** 恢复默认布局（当前未接入 UI，保留给后续「重置布局」用） */
export function resetLayout() {
  leftPanelWidth.value = DEFAULTS.leftPanelWidth
  branchPanelFlex.value = DEFAULTS.branchPanelFlex
  fileListFlex.value = DEFAULTS.fileListFlex
  diffAreaFlex.value = DEFAULTS.diffAreaFlex
  logAreaFlex.value = DEFAULTS.logAreaFlex
  flushLayout()
}

export function useLayout() {
  return { leftPanelWidth, branchPanelFlex, fileListFlex, diffAreaFlex, logAreaFlex, resetLayout, flushLayout }
}
