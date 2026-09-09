<script setup lang="ts">
import { ref, computed, watch, nextTick, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import type { FileDiff, DiffLine } from '../types'
import { fileLang, tokenizeLine, type Token } from '../syntaxHighlight'
import { computeAnchoredScrollTop } from '../diffScrollSync'
import {
  Save, Pencil, History,
  RotateCcw, ChevronUp, ChevronDown,
  ArrowLeftToLine, Copy,
} from 'lucide-vue-next'
import { Button } from '@/components/ui/button'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip'
import { writeFileContent } from '../api/git'

interface Props {
  repoPath: string
  filePath: string
  diff: FileDiff
  // 修改文件列表 + 当前下标，用于「上一个/下一个文件」跳转；
  // 单文件打开时不传，相关按钮 disabled
  filesList?: string[]
  currentIndex?: number
}

const props = defineProps<Props>()
const emit = defineEmits(['save', 'close', 'navigate-file', 'reload'])
const { t } = useI18n()

// 语法高亮语言（按文件路径扩展名推断），用于编辑窗口逐行关键字着色
const lang = computed(() => fileLang(props.filePath))

// ---- 行对齐模型：左(旧)/右(新) 按 diff 逐行对齐，复刻「查看差异面板」的行号与对比 ----
// 每一行对应 diff 的一个对齐单元：eq 未改 / del 旧版有·新版无 / add 新版有·旧版无 / mod 两侧配对修改
interface AlignedRow {
  type: 'eq' | 'del' | 'add' | 'mod'
  oldIdx: number | null
  newIdx: number | null
}

// 由后端返回的 diff.lines 构造对齐行（与 DiffViewer 共用同一份后端 diff，保证编辑窗口
// 与查看面板的差异标注【像素级一致】）。后端已是 Myers 差分（O(ND)），几万行也能算出，
// 前端不再跑 O(m*n) 的 LCS DP（那才是超大文件卡死/白屏的根因）。
// 后端已把相邻 delete+add 合并为 modified 行，直接按 line_type 映射：
//   context→eq / delete→del / add→add / modified→mod
function buildAlignmentFromLines(lines: DiffLine[]): AlignedRow[] {
  return lines.map((l): AlignedRow => {
    switch (l.line_type) {
      case 'delete':
        return { type: 'del', oldIdx: l.old_line != null ? l.old_line - 1 : null, newIdx: null }
      case 'add':
        return { type: 'add', oldIdx: null, newIdx: l.new_line != null ? l.new_line - 1 : null }
      case 'modified':
        return { type: 'mod', oldIdx: l.old_line != null ? l.old_line - 1 : null, newIdx: l.new_line != null ? l.new_line - 1 : null }
      case 'context':
      default:
        return { type: 'eq', oldIdx: l.old_line != null ? l.old_line - 1 : null, newIdx: l.new_line != null ? l.new_line - 1 : null }
    }
  })
}

// 构造对齐行：优先用后端 diff.lines（含真实差异标注）；lines 为空（超大 oversized / 二进制）
// 时退化逐行 eq 对齐（行数不一致按最大行数补 eq 行），保证编辑视图仍可编辑保存。
function buildAlignment(oldL: string[], newL: string[], lines: DiffLine[]): AlignedRow[] {
  if (lines.length > 0) {
    return buildAlignmentFromLines(lines)
  }
  const len = Math.max(oldL.length, newL.length)
  const rows: AlignedRow[] = []
  for (let k = 0; k < len; k++) {
    const oldIdx = k < oldL.length ? k : null
    const newIdx = k < newL.length ? k : null
    rows.push({ type: 'eq', oldIdx, newIdx })
  }
  return rows
}

// 行级对齐结构：仅在第一遍（old vs new）计算一次，行数 K 固定。
// 右栏编辑后不再二次 buildAlignment —— 二次对齐会因「删除行占位空字符串」等
// 原因导致行数漂移（K2≠K1），使左右 textarea 行数不等、右栏 maxScroll 偏小、
// 拖到底时右栏内容显示不全。统一以这一次对齐的行数为准，右栏文本按 index 对应。
const alignment = ref<AlignedRow[]>(
  buildAlignment(props.diff.old_content, props.diff.new_content, props.diff.lines),
)
// 右侧可编辑内容初始为「对齐后的新版本」：删除行处留空行占位，与差异面板对齐一致。
// 行数严格 = alignment.length（K），保证左右 textarea 行数一致、滚动同步正确。
const content = ref(
  alignment.value
    .map((r) => (r.newIdx !== null ? (props.diff.new_content[r.newIdx] ?? '') : ''))
    .join('\n'),
)
const isSaving = ref(false)

const leftTextarea = ref<HTMLTextAreaElement | null>(null)
const rightTextarea = ref<HTMLTextAreaElement | null>(null)
const leftLinesRef = ref<HTMLElement | null>(null)
const rightLinesRef = ref<HTMLElement | null>(null)

// 右侧当前内容行（防抖 50ms，避免每次按键都重算）。newLinesRef 与 alignment 等长、
// 按 index 一一对应，供 rowModel 取右栏文本（不再用 newIdx 间接取，避免错位）
const newLinesRef = ref<string[]>(content.value.split('\n'))
let newLinesTimer: number | null = null
watch(content, (val) => {
  if (newLinesTimer !== null) window.clearTimeout(newLinesTimer)
  newLinesTimer = window.setTimeout(() => {
    newLinesRef.value = val.split('\n')
  }, 50)
})

// 字符级差异着色已移除：差异仅用行级底色表达，文字改为按代码关键字着色（见 tokenizeLine）。

// 逐行渲染模型：左/右文本、行号(old/new)、底色、字符级分段
const rowModel = computed(() => {
  const oldL = props.diff.old_content
  const newL = newLinesRef.value
  return alignment.value.map((r, i) => {
    const oldText = r.oldIdx !== null ? (oldL[r.oldIdx] ?? '') : ''
    // 右栏文本按对齐行 index 取（content/newLinesRef 与 alignment 等长、一一对应），
    // 不再用 r.newIdx 间接取原 new_content —— 那样在 K1≠K2 时会错位
    const newText = newL[i] ?? ''
    // 轻量重判行级类型（基于左右当前文本），既反映「用户编辑后」的真实差异状态，
    // 又避免二次 buildAlignment 带来的行数漂移（K2≠K1）
    //
    // ⚠️ 判据按「该行在 alignment 中的角色」区分，不能拿 newText 是否为空一刀切：
    // 两侧都真实存在的【相同空行】（oldText==='' 且 newText===''）不是差异，
    // 若用 newText === '' → del 的旧逻辑，会把文件里所有共同空行误标成删除（红底），
    // 与查看面板（Rust lcs_diff 判 context）不一致。
    let type: AlignedRow['type']
    if (r.oldIdx !== null && r.newIdx !== null) {
      // eq / mod 行：左右都有真实行号 → 文本相同为 eq，否则 mod（含清空后的改删场景）
      type = oldText === newText ? 'eq' : 'mod'
    } else if (r.oldIdx !== null) {
      // 原删除行（右栏为占位空行）：右栏补了内容则按内容归 eq/mod，仍空则保持 del
      type = newText === '' ? 'del' : (oldText === newText ? 'eq' : 'mod')
    } else {
      // 原新增行（含新增空行）：保持 add
      type = 'add'
    }
    // 语法高亮：按关键字着色（IDEA 风格），不再做字符级差异着色
    let leftSegs: Token[] = []
    let rightSegs: Token[] = []
    if (r.oldIdx !== null) leftSegs = tokenizeLine(oldText, lang.value)
    if (r.newIdx !== null) rightSegs = tokenizeLine(newText, lang.value)
    // eq 行无底色；del/add/mod 行按差异类型上底色（与差异面板一致：删红·加绿·改黄）
    const leftCls = type === 'eq' ? '' : 'hl-' + type
    const rightCls = type === 'eq' ? '' : 'hl-' + type
    return {
      oldText,
      newText,
      leftSegs,
      rightSegs,
      leftCls,
      rightCls,
      oldNum: r.oldIdx !== null ? r.oldIdx + 1 : null,
      newNum: r.newIdx !== null ? r.newIdx + 1 : null,
      showLeft: r.oldIdx !== null,
      showRight: r.newIdx !== null,
      type,
    }
  })
})

// 智能锚定滚动同步用的「锚点行」：两侧都有内容的行（eq / mod）才作对齐锚点，
// 新增行（左空）/ 删除行（右空）不是锚点 —— 与 DiffViewer 共用 src/diffScrollSync.ts。
const anchorFlags = computed(() =>
  rowModel.value.map((r) => r.type === 'eq' || r.type === 'mod'),
)

// 左栏只读文本（新增行处留空行，与右栏对齐）
const leftAlignedText = computed(() => rowModel.value.map((r) => r.oldText).join('\n'))

// ---- 右侧概览标尺（overview ruler）：叠在垂直滚动条上纯展示差异位置（与查看面板一致）----
const rightScrollInfo = ref({ top: 0, clientH: 1, scrollH: 1 })
function updateRightScrollInfo() {
  const el = rightTextarea.value
  if (!el) return
  rightScrollInfo.value = { top: el.scrollTop, clientH: el.clientHeight, scrollH: el.scrollHeight }
}

const rulerMarks = computed(() => {
  const total = rowModel.value.length || 1
  const marks: Array<{ top: string; height: string; cls: string }> = []
  rowModel.value.forEach((row, i) => {
    if (row.type === 'eq') return
    marks.push({
      top: (i / total) * 100 + '%',
      height: `max(2px, ${(1 / total) * 100}%)`,
      cls: 'ruler-' + row.type,
    })
  })
  return marks
})

// overlay 内层 ref，用于跟随滚动做 translateY
const leftOverlayInner = ref<HTMLElement | null>(null)
const rightOverlayInner = ref<HTMLElement | null>(null)
const middleActionsRef = ref<HTMLElement | null>(null)

// 将连续同类型差异行（del/add/mod）合并为一组，每组只渲染一组操作按钮
// 相同行（eq）不合并，每行占位保持对齐
interface ActionGroup {
  type: 'eq' | 'del' | 'add' | 'mod'
  startIdx: number   // 组起始行在 rowModel 中的 index
  endIdx: number     // 组结束行的下一个 index（不含），endIdx - startIdx = 行数
  isDiff: boolean    // 是否差异行
}

const groupedActions = computed<ActionGroup[]>(() => {
  const groups: ActionGroup[] = []
  const rows = rowModel.value
  let i = 0
  while (i < rows.length) {
    const row = rows[i]
    if (row.type === 'eq') {
      // 相同行：每行独立一组，保持行高对齐
      groups.push({ type: 'eq', startIdx: i, endIdx: i + 1, isDiff: false })
      i++
    } else {
      // 差异行：合并连续同类型
      const t = row.type
      let j = i + 1
      while (j < rows.length && rows[j].type === t) j++
      groups.push({ type: t, startIdx: i, endIdx: j, isDiff: true })
      i = j
    }
  }
  return groups
})

// ---- IDEA / Beyond Compare 风格差异连接带 ----
// 把连续差异行跨类型合并为一个 hunk，分别取左/右侧「有内容行」的视觉纵向范围，
// 在中间列画弧形四边形（S 曲线上下沿）。两侧行数不同时形成梯形/漏斗，
// 与截图工具一致；纯新增/删除时无内容一侧塌缩为块中点。
interface DiffHunk {
  kind: 'del' | 'add' | 'mod'
  path: string
}

const hunks = computed<DiffHunk[]>(() => {
  const rows = rowModel.value
  const result: DiffHunk[] = []
  const x0 = 0.75          // 左边缘（内缩避免描边裁剪）
  const x1 = 34 - 0.75     // 右边缘（与 viewBox 宽 34 一致）
  const c = 34 / 2         // 贝塞尔控制点 x：两端水平切线
  let i = 0
  while (i < rows.length) {
    if (rows[i].type === 'eq') { i++; continue }
    let j = i + 1
    while (j < rows.length && rows[j].type !== 'eq') j++
    // hunk = [i, j)
    let lFirst = -1, lLast = -1, rFirst = -1, rLast = -1
    let hasDel = false, hasMod = false
    for (let k = i; k < j; k++) {
      const r = rows[k]
      if (r.showLeft) { if (lFirst < 0) lFirst = k; lLast = k }
      if (r.showRight) { if (rFirst < 0) rFirst = k; rLast = k }
      if (r.type === 'del') hasDel = true
      else if (r.type === 'mod') hasMod = true
    }
    const mid = (i * 18 + j * 18) / 2 // 无内容侧塌缩点：块垂直中点
    const lt = lFirst >= 0 ? lFirst * 18 : mid
    const lb = lFirst >= 0 ? (lLast + 1) * 18 : mid
    const rt = rFirst >= 0 ? rFirst * 18 : mid
    const rb = rFirst >= 0 ? (rLast + 1) * 18 : mid
    const kind: DiffHunk['kind'] = hasMod ? 'mod' : hasDel ? 'del' : 'add'
    const path = [
      `M ${x0} ${lt}`,
      `C ${c} ${lt} ${c} ${rt} ${x1} ${rt}`,
      `L ${x1} ${rb}`,
      `C ${c} ${rb} ${c} ${lb} ${x0} ${lb}`,
      'Z',
    ].join(' ')
    result.push({ kind, path })
    i = j
  }
  return result
})

// 「上一个/下一个文件」导航：filesList 为空时禁用，边界自动 disabled
const hasFilesNav = computed(() => Array.isArray(props.filesList) && props.filesList.length > 1)
const currentIdx = computed(() => (typeof props.currentIndex === 'number' && props.currentIndex >= 0 ? props.currentIndex : -1))
const totalFiles = computed(() => (Array.isArray(props.filesList) ? props.filesList.length : 0))
const canPrev = computed(() => hasFilesNav.value && currentIdx.value > 0)
const canNext = computed(() => hasFilesNav.value && currentIdx.value >= 0 && currentIdx.value < totalFiles.value - 1)
const positionLabel = computed(() => {
  if (!hasFilesNav.value || currentIdx.value < 0) return ''
  return `${currentIdx.value + 1} / ${totalFiles.value}`
})

// 本地轻量通知（避免引入额外浮层依赖），2.5 秒后自动消失
const localToast = ref('')
let toastTimer: number | null = null
function flashToast(msg: string) {
  localToast.value = msg
  if (toastTimer !== null) window.clearTimeout(toastTimer)
  toastTimer = window.setTimeout(() => {
    localToast.value = ''
    toastTimer = null
  }, 2500)
}

watch(
  () => props.diff,
  (d) => {
    const a = buildAlignment(d.old_content, d.new_content, d.lines)
    alignment.value = a
    content.value = a.map((r) => (r.newIdx !== null ? (d.new_content[r.newIdx] ?? '') : '')).join('\n')
    newLinesRef.value = content.value.split('\n')
    scrollToFirstDiff()
  },
)

// 智能锚定滚动同步（Beyond Compare 风格），与 DiffViewer 共用 src/diffScrollSync.ts。
// 程序化回写会引发对端 textarea 的 scroll 回响，用「最近一次程序化写入的目标元素 + 120ms 窗口」
// 吞掉，避免非 1:1 锚定映射下的反向改写 / 乒乓错位。
let programmaticTarget: HTMLTextAreaElement | null = null
let programmaticTime = 0

function syncScroll(source: 'left' | 'right') {
  const refs: Record<string, HTMLTextAreaElement | null> = {
    left: leftTextarea.value,
    right: rightTextarea.value,
  }
  const lineRefs: Record<string, HTMLElement | null> = {
    left: leftLinesRef.value,
    right: rightLinesRef.value,
  }
  const overlayInners: Record<string, HTMLElement | null> = {
    left: leftOverlayInner.value,
    right: rightOverlayInner.value,
  }
  const sourceEl = refs[source]
  if (!sourceEl) return

  // 吞掉「我们刚程序化写入」触发回来的 scroll（对端回响），否则会反向改写源侧、产生错位
  if (programmaticTarget === sourceEl && performance.now() - programmaticTime < 120) {
    programmaticTarget = null
    return
  }

  const sourceTop = sourceEl.scrollTop
  const sourceLeft = sourceEl.scrollLeft
  const targetTop = computeAnchoredScrollTop(sourceTop, anchorFlags.value, 18)

  ;(Object.keys(refs) as Array<keyof typeof refs>).forEach((key) => {
    const el = refs[key]
    const lineEl = lineRefs[key]
    const ov = overlayInners[key]
    const top = key === source ? sourceTop : targetTop
    if (el && el !== sourceEl) {
      if (el.scrollTop !== top) {
        programmaticTarget = el
        programmaticTime = performance.now()
        el.scrollTop = top
      }
      if (el.scrollLeft !== sourceLeft) el.scrollLeft = sourceLeft
    }
    if (lineEl) lineEl.scrollTop = top
    if (ov) {
      ov.style.transform = `translate(${-sourceLeft}px, ${-top}px)`
    }
  })
  // 中间操作列跟随「源侧」真实位置：按钮始终贴着正在滚动、有内容的一侧
  const midEl = middleActionsRef.value
  if (midEl) {
    midEl.scrollTop = sourceTop
  }
  if (source === 'right') updateRightScrollInfo()
}

/**
 * 字符 idx 换算为行号（0-based）。
 * 输入 textarea 的 selectionStart，直接换算成第几行。
 */
function getCaretLine(textarea: HTMLTextAreaElement): number {
  const text = textarea.value
  const cursor = Math.max(0, Math.min(textarea.selectionStart ?? 0, text.length))
  let line = 0
  for (let i = 0; i < cursor; i++) {
    if (text.charCodeAt(i) === 10) line++ // '\n'
  }
  return line
}

/** 移动光标到右侧指定行（0-based），用于覆盖后让用户看到落点。 */
function focusRightLine(lineIdx: number) {
  nextTick(() => {
    if (!rightTextarea.value) return
    let pos = 0
    const lines = rightTextarea.value.value.split('\n')
    for (let i = 0; i < lineIdx && i < lines.length; i++) pos += lines[i].length + 1
    rightTextarea.value.focus()
    rightTextarea.value.setSelectionRange(pos, pos)
  })
}

async function handleSave() {
  if (isSaving.value) return
  isSaving.value = true
  try {
    // 用稳定的 alignment（第一遍对齐，行数固定 K）还原真实新文件，避免二次
    // buildAlignment 在「删除行空占位」下产生行数漂移（K2≠K1）导致内容错位/丢行。
    // 过滤规则按「行在 alignment 中的角色」判定，不能拿内容是否为空一刀切：
    // - newIdx 非空 = 右侧真实存在的行：原样写入（含真实空行——否则保存后文件里
    //   所有共同空行/新增空行都会被删光，凭空多出一堆“删空行”差异）
    // - newIdx 为空 = 删除行右侧的空占位：仅当用户在该行键入了内容才写入
    const lines = content.value.split('\n')
    const align = alignment.value
    const saved: string[] = []
    align.forEach((r, idx) => {
      const line = lines[idx] ?? ''
      if (r.newIdx !== null || line !== '') saved.push(line)
    })
    await writeFileContent(props.repoPath, props.filePath, saved.join('\n'))
    emit('save')
  } catch (e) {
    console.error('Save file error:', e)
    flashToast(t('diffEditor.saveFailed', { error: typeof e === 'string' ? e : (e as any)?.toString?.() || String(e) }))
  } finally {
    isSaving.value = false
  }
}

/** 重新加载：从 Rust 重新拉 diff，丢弃右侧编辑区未保存内容。 */
function handleReload() {
  if (!confirm(t('diffEditor.reloadConfirm'))) return
  emit('reload')
}

/** 上一个 / 下一个文件：父组件负责 close 当前窗口 + 重开目标文件。 */
function navigatePrev() {
  if (!canPrev.value) return
  emit('navigate-file', currentIdx.value - 1)
}
function navigateNext() {
  if (!canNext.value) return
  emit('navigate-file', currentIdx.value + 1)
}

/** 接受左侧按钮的 tooltip 文案 */
function getAcceptLeftTitle(type: string): string {
  switch (type) {
    case 'del': return t('diffEditor.acceptLeftDel')
    case 'add': return t('diffEditor.acceptLeftAdd')
    case 'mod': return t('diffEditor.acceptLeftMod')
    default: return t('diffEditor.acceptLeftDefault')
  }
}

/** 接受右侧按钮的 tooltip 文案 */
function getAcceptRightTitle(type: string): string {
  switch (type) {
    case 'del': return t('diffEditor.acceptRightDel')
    case 'add': return t('diffEditor.acceptRightAdd')
    case 'mod': return t('diffEditor.acceptRightMod')
    default: return t('diffEditor.acceptRightDefault')
  }
}
function handleAcceptLeft(groupStartIdx: number) {
  const lines = content.value.split('\n')
  let count = 0
  let i = groupStartIdx
  const groupType = alignment.value[groupStartIdx]?.type
  if (!groupType) return
  while (i < alignment.value.length && alignment.value[i].type === groupType) {
    const row = alignment.value[i]
    if (i >= lines.length) { i++; continue }
    if (groupType === 'del' && row.oldIdx !== null) {
      // del 行：恢复 HEAD 版本（从删除恢复）
      lines[i] = props.diff.old_content[row.oldIdx]
      count++
    } else if (groupType === 'add') {
      // add 行：撤销新增（清空该行）
      lines[i] = ''
      count++
    } else if (groupType === 'mod' && row.oldIdx !== null) {
      // mod 行：用 HEAD 版本覆盖
      lines[i] = props.diff.old_content[row.oldIdx]
      count++
    }
    i++
  }
  if (count > 0) {
    content.value = lines.join('\n')
    flashToast(t('diffEditor.acceptedLeft', { n: count }))
  }
}

/** 接受右侧版本（支持整组） */
function handleAcceptRight(groupStartIdx: number) {
  const lines = content.value.split('\n')
  const groupType = alignment.value[groupStartIdx]?.type
  if (!groupType) return
  let count = 0
  let i = groupStartIdx
  while (i < alignment.value.length && alignment.value[i].type === groupType) {
    const row = alignment.value[i]
    if (row.type === 'del' && i < lines.length) {
      // 接受删除：清空右侧对应行
      lines[i] = ''
      count++
    } else if (row.type === 'add') {
      // 接受新增：保持原样
      count++
    } else if (row.type === 'mod') {
      // 接受右侧：保持原样
      count++
    }
    i++
  }
  if (count > 0) {
    content.value = lines.join('\n')
    flashToast(t('diffEditor.acceptedRight', { n: count }))
  }
}

/** 跳转到上一处差异（非 eq 行） */
function jumpToPrevDiff() {
  const cursor = getCaretLine(rightTextarea.value!)
  const rows = rowModel.value
  for (let i = cursor - 1; i >= 0; i--) {
    if (rows[i]?.type !== 'eq') {
      focusRightLine(i)
      scrollToRow(i)
      return
    }
  }
  flashToast(t('diffEditor.atFirstDiff'))
}

/** 跳转到下一处差异 */
function jumpToNextDiff() {
  const cursor = getCaretLine(rightTextarea.value!)
  const rows = rowModel.value
  for (let i = cursor + 1; i < rows.length; i++) {
    if (rows[i]?.type !== 'eq') {
      focusRightLine(i)
      scrollToRow(i)
      return
    }
  }
  flashToast(t('diffEditor.atLastDiff'))
}

/** 滚动到指定行（0-based） */
function scrollToRow(rowIdx: number) {
  const ta = rightTextarea.value
  if (!ta) return
  const lineH = 18
  const padTop = 8
  const targetTop = Math.max(0, rowIdx * lineH - padTop)
  ta.scrollTop = targetTop
  syncScroll('right')
}

/** 统计差异行数量（供导航按钮显示） */
const diffLineCount = computed(() => rowModel.value.filter((r) => r.type !== 'eq').length)
const currentDiffIdx = computed(() => {
  const rows = rowModel.value
  let cursor = -1
  if (rightTextarea.value) cursor = getCaretLine(rightTextarea.value)
  let count = 0
  for (let i = 0; i <= cursor && i < rows.length; i++) {
    if (rows[i].type !== 'eq') count++
  }
  return count
})
function handleCopyFromLeft() {
  if (!leftTextarea.value || !rightTextarea.value) return
  const leftLineIdx = getCaretLine(leftTextarea.value)
  const row = alignment.value[leftLineIdx]
  if (!row || row.oldIdx === null) return
  const newLine = props.diff.old_content[row.oldIdx]

  const rightLines = content.value.split('\n')
  const rightLineIdx = getCaretLine(rightTextarea.value)
  if (rightLineIdx >= rightLines.length) return

  rightLines[rightLineIdx] = newLine
  content.value = rightLines.join('\n')
  flashToast(t('diffEditor.copiedFromLeft', { left: row.oldIdx + 1, right: rightLineIdx + 1 }))
  focusRightLine(rightLineIdx)
}

/**
 * 「右加」= 复制右侧光标行到剪贴板。
 * 左侧是 HEAD 版本，不可写入，所以右向覆盖无意义，改为剪贴板导出。
 */
async function handleCopyRightToClipboard() {
  if (!rightTextarea.value) return
  const rightLineIdx = getCaretLine(rightTextarea.value)
  const rightLines = content.value.split('\n')
  if (rightLineIdx >= rightLines.length) return
  const lineText = rightLines[rightLineIdx]
  try {
    await navigator.clipboard.writeText(lineText)
    flashToast(t('diffEditor.copiedRightLine', { line: rightLineIdx + 1 }))
  } catch (e) {
    console.error('复制到剪贴板失败:', e)
    flashToast(t('diffEditor.copyFailed'))
  }
}

function getFileName(path: string): string {
  const parts = path.replace(/\\/g, '/').split('/')
  return parts[parts.length - 1] || path
}

/** 自动定位到第一个差异行：打开文件或切换文件时，让视图直接落在第一处差异。 */
function scrollToFirstDiff() {
  const firstIdx = rowModel.value.findIndex((r) => r.type !== 'eq')
  if (firstIdx < 0) return
  let tries = 0
  const apply = () => {
    const ta = rightTextarea.value
    if (!ta) return
    // Tauri WebView2 冷启动 layout 可能很慢：scrollHeight 未就绪（=0）时设 scrollTop 会被
    // 浏览器 clamp 为 0，看起来"没滚动"。用 setTimeout 兜底重试（隐藏/未 paint 的 webview
    // 中 setTimeout 也按时触发，而 requestAnimationFrame 可能不触发），最多重试 20 次（2s）。
    if (ta.scrollHeight === 0 && tries < 20) {
      tries++
      window.setTimeout(apply, 100)
      return
    }
    updateRightScrollInfo()
    const lineH = 18
    const padTop = 8
    const targetTop = Math.max(0, firstIdx * lineH - padTop)
    ta.scrollTop = targetTop
    syncScroll('right')
    focusRightLine(firstIdx)
    // focus()/setSelectionRange() 可能触发浏览器"滚动光标入视口"把 scrollTop 改走
    // （未激活的 textarea 先设 selection 无效、focus 时默认光标在末尾 → 会滚到文件尾），
    // 钉回目标位置；setTimeout 在隐藏/未 paint 的 webview 也按时触发
    window.setTimeout(() => {
      const t2 = rightTextarea.value
      if (!t2) return
      t2.scrollTop = targetTop
      syncScroll('right')
    }, 0)
  }
  // nextTick 保证 Vue 完成 DOM 更新；scrollHeight 就绪前的重试交给 setTimeout
  nextTick(apply)
}

onMounted(() => {
  scrollToFirstDiff()
})
</script>

<template>
  <div class="diff-editor">
    <TooltipProvider :delay-duration="260">
      <div class="editor-toolbar">
        <!-- 左：标题 + 文件名 -->
        <div class="editor-title">
          <Pencil :size="14" class="title-icon" />
          <span>{{ t('diffEditor.editFile') }}</span>
          <span class="file-name">{{ getFileName(filePath) }}</span>
        </div>

        <!-- 中：文件间导航（filesList 有内容时才出现） -->
        <div v-if="hasFilesNav" class="editor-nav">
          <Tooltip>
            <TooltipTrigger as-child>
              <Button
                variant="ghost"
                size="icon-sm"
                class="h-8 w-8"
                :aria-label="t('diffEditor.prevFile')"
                :disabled="!canPrev"
                @click="navigatePrev"
              >
                <ChevronUp :size="15" />
              </Button>
            </TooltipTrigger>
            <TooltipContent side="bottom">{{ t('diffEditor.prevFileTip') }}</TooltipContent>
          </Tooltip>
          <span class="position-label" role="status" :aria-label="t('diffEditor.currentFilePos')">{{ positionLabel }}</span>
          <Tooltip>
            <TooltipTrigger as-child>
              <Button
                variant="ghost"
                size="icon-sm"
                class="h-8 w-8"
                :aria-label="t('diffEditor.nextFile')"
                :disabled="!canNext"
                @click="navigateNext"
              >
                <ChevronDown :size="15" />
              </Button>
            </TooltipTrigger>
            <TooltipContent side="bottom">{{ t('diffEditor.nextFileTip') }}</TooltipContent>
          </Tooltip>
        </div>

        <!-- 图例：差异行着色含义 -->
        <div class="diff-legend">
          <span class="legend-item"><i class="legend-dot del" />{{ t('diffEditor.legendDel') }}</span>
          <span class="legend-item"><i class="legend-dot add" />{{ t('diffEditor.legendAdd') }}</span>
          <span class="legend-item"><i class="legend-dot mod" />{{ t('diffEditor.legendMod') }}</span>
        </div>

        <!-- 右：操作组（tool-cluster 风格，与主工具栏统一） -->
        <div class="editor-actions">
          <Tooltip>
            <TooltipTrigger as-child>
              <Button variant="ghost" size="icon-sm" class="h-8 w-8" :aria-label="t('diffEditor.reload')" @click="handleReload">
                <RotateCcw :size="14" />
              </Button>
            </TooltipTrigger>
            <TooltipContent side="bottom">{{ t('diffEditor.reloadTip') }}</TooltipContent>
          </Tooltip>

          <Tooltip>
            <TooltipTrigger as-child>
              <Button variant="ghost" size="icon-sm" class="h-8 w-8" :aria-label="t('diffEditor.leftAddAria')" @click="handleCopyFromLeft">
                <ArrowLeftToLine :size="14" />
              </Button>
            </TooltipTrigger>
            <TooltipContent side="bottom">{{ t('diffEditor.leftAddTip') }}</TooltipContent>
          </Tooltip>

          <Tooltip>
            <TooltipTrigger as-child>
              <Button variant="ghost" size="icon-sm" class="h-8 w-8" :aria-label="t('diffEditor.rightAddAria')" @click="handleCopyRightToClipboard">
                <Copy :size="14" />
              </Button>
            </TooltipTrigger>
            <TooltipContent side="bottom">{{ t('diffEditor.rightAddTip') }}</TooltipContent>
          </Tooltip>

          <Button variant="ghost" size="sm" class="save-btn" @click="handleSave" :disabled="isSaving">
            <Save :size="14" />
            {{ isSaving ? t('diffEditor.saving') : t('diffEditor.save') }}
          </Button>
        </div>
      </div>

      <!-- 轻量本地通知（无外部 toast 依赖） -->
      <div v-if="localToast" class="local-toast" role="status">{{ localToast }}</div>
    </TooltipProvider>

    <div class="editor-panels">
      <div class="editor-panel">
        <div class="panel-label">
          <History :size="11" />
          <span>{{ t('diffEditor.oldHead') }}</span>
        </div>
        <div class="editor-container">
          <div ref="leftLinesRef" class="line-numbers">
            <div v-for="(row, idx) in rowModel" :key="'gl' + idx" class="line-num">{{ row.oldNum ?? '' }}</div>
          </div>
          <div class="textarea-wrapper">
            <div class="diff-overlay" aria-hidden="true">
              <div ref="leftOverlayInner" class="diff-overlay-inner">
                <div
                  v-for="(row, idx) in rowModel"
                  :key="'lo' + idx"
                  class="hl-line"
                  :class="row.leftCls"
                ><span
                    v-for="(seg, si) in row.leftSegs"
                    :key="si"
                    :class="'tok-' + seg.type"
                  >{{ seg.text }}</span></div>
              </div>
            </div>
            <textarea
              ref="leftTextarea"
              :value="leftAlignedText"
              readonly
              class="editor-textarea"
              spellcheck="false"
              @scroll="syncScroll('left')"
            />
          </div>
        </div>
      </div>

      <!-- 中间操作列：差异行级操作按钮 + 导航 -->
      <div class="editor-middle-column">
        <!-- 顶部导航：上一处/下一处差异 -->
        <div class="middle-nav">
          <Tooltip>
            <TooltipTrigger as-child>
              <Button
                variant="ghost"
                size="icon-sm"
                class="h-6 w-6"
                :disabled="diffLineCount === 0"
                @click="jumpToPrevDiff"
              >
                <ChevronUp :size="14" />
              </Button>
            </TooltipTrigger>
            <TooltipContent side="bottom">{{ t('diffEditor.prevDiff') }}</TooltipContent>
          </Tooltip>
          <span v-if="diffLineCount > 0" class="diff-count">{{ currentDiffIdx }}/{{ diffLineCount }}</span>
          <Tooltip>
            <TooltipTrigger as-child>
              <Button
                variant="ghost"
                size="icon-sm"
                class="h-6 w-6"
                :disabled="diffLineCount === 0"
                @click="jumpToNextDiff"
              >
                <ChevronDown :size="14" />
              </Button>
            </TooltipTrigger>
            <TooltipContent side="bottom">{{ t('diffEditor.nextDiff') }}</TooltipContent>
          </Tooltip>
        </div>

        <!-- 行级操作按钮（VS Code Merge Editor 风格：箭头 + 颜色区块） -->
        <div class="middle-actions" ref="middleActionsRef">
          <!-- IDEA / Beyond Compare 风格连接带图层：弧形四边形把左右差异块桥接起来，
               绝对定位于滚动内容顶部，随中间列一起滚动（纯装饰，不拦截事件） -->
          <svg
            v-if="rowModel.length > 0"
            class="diff-bands"
            :viewBox="`0 0 34 ${rowModel.length * 18}`"
            :style="{ height: rowModel.length * 18 + 'px' }"
            preserveAspectRatio="none"
            aria-hidden="true"
          >
            <path
              v-for="(h, hi) in hunks"
              :key="'hb' + hi"
              class="band"
              :class="'band-' + h.kind"
              :d="h.path"
              vector-effect="non-scaling-stroke"
            />
          </svg>
          <div
            v-for="(group, gi) in groupedActions"
            :key="'ga' + gi"
            class="action-group"
            :class="[`group-${group.type}`, { 'is-diff': group.isDiff }]"
            :style="{ height: ((group.endIdx - group.startIdx) * 18) + 'px' }"
          >
            <!-- 相同行：占位 -->
            <template v-if="!group.isDiff">
              <span class="mid-placeholder"></span>
            </template>

            <!-- 差异行组：VS Code 风格箭头按钮，绝对定位垂直居中 -->
            <template v-else>
              <div class="merge-buttons" :style="{ top: '50%', transform: 'translateY(-50%)' }">
                <button
                  class="merge-btn accept-left"
                  :title="getAcceptLeftTitle(group.type)"
                  @click="handleAcceptLeft(group.startIdx)"
                >
                  <svg class="arrow-icon" viewBox="0 0 16 16" fill="currentColor" width="14" height="14">
                    <path d="M10 3L5 8l5 5V3z" transform="rotate(180 8 8)"/>
                  </svg>
                </button>
                <button
                  class="merge-btn accept-right"
                  :title="getAcceptRightTitle(group.type)"
                  @click="handleAcceptRight(group.startIdx)"
                >
                  <svg class="arrow-icon" viewBox="0 0 16 16" fill="currentColor" width="14" height="14">
                    <path d="M10 3L5 8l5 5V3z"/>
                  </svg>
                </button>
              </div>
            </template>
          </div>
        </div>
      </div>

      <div class="editor-panel">
        <div class="panel-label">
          <Pencil :size="11" />
          <span>{{ t('diffEditor.newWorking') }}</span>
        </div>
        <div class="editor-container">
          <div ref="rightLinesRef" class="line-numbers">
            <div v-for="(row, idx) in rowModel" :key="'gr' + idx" class="line-num">{{ row.newNum ?? '' }}</div>
          </div>
          <div class="textarea-wrapper">
            <div class="diff-overlay" aria-hidden="true">
              <div ref="rightOverlayInner" class="diff-overlay-inner">
                <div
                  v-for="(row, idx) in rowModel"
                  :key="'ro' + idx"
                  class="hl-line"
                  :class="row.rightCls"
                ><span
                    v-for="(seg, si) in row.rightSegs"
                    :key="si"
                    :class="'tok-' + seg.type"
                  >{{ seg.text }}</span></div>
              </div>
            </div>
            <textarea
              ref="rightTextarea"
              id="right-editor"
              v-model="content"
              class="editor-textarea"
              spellcheck="false"
              @scroll="syncScroll('right')"
            />
          </div>
          <!-- 右侧概览标尺：叠在垂直滚动条上纯展示（与查看差异面板一致） -->
          <div class="diff-ruler" aria-hidden="true">
            <div
              v-for="(m, idx) in rulerMarks"
              :key="idx"
              class="ruler-mark"
              :class="m.cls"
              :style="{ top: m.top, height: m.height }"
            />
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.diff-editor {
  display: flex;
  flex-direction: column;
  height: 100%;
  background-color: var(--bg-secondary);
  min-height: 0;
}

.editor-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  height: 44px;
  padding: 0 10px;
  background-color: var(--bg-toolbar);
  border-bottom: 1px solid var(--border-color);
  flex-shrink: 0;
}

.editor-title {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  font-weight: 600;
  color: var(--text-secondary);
  min-width: 0;
}

.title-icon {
  color: var(--accent-text);
}

.file-name {
  font-family: Consolas, Monaco, monospace;
  color: var(--accent-text);
  font-weight: 500;
}

/* 文件间导航：上/下按钮 + 位置标签（tool-cluster 风格，与主工具栏统一） */
.editor-nav {
  display: flex;
  align-items: center;
  gap: 2px;
  padding: 3px;
  border: 1px solid var(--border-light);
  border-radius: 9px;
  background-color: var(--bg-primary);
}

.position-label {
  font-size: 11px;
  font-family: Consolas, Monaco, monospace;
  color: var(--text-secondary);
  min-width: 36px;
  text-align: center;
  user-select: none;
}

.editor-actions {
  display: flex;
  align-items: center;
  gap: 2px;
  padding: 3px;
  border-radius: 9px;
  background-color: var(--bg-primary);
  border: 1px solid var(--border-light);
}

/* 主操作：保存按钮，沿用品牌主色文字柔和强调（与 ToolBar seg-commit 一致），
   不抢实心亮色，与同组 ghost 图标按钮同属一套视觉语言 */
.editor-actions :deep(.save-btn) {
  height: 32px;
  padding: 0 12px;
  gap: 6px;
  font-size: 12px;
  font-weight: 500;
  color: var(--brand-primary);
}

.editor-actions :deep(.save-btn:hover):not(:disabled) {
  background-color: var(--bg-hover);
  color: var(--brand-primary);
}

.editor-actions :deep(.save-btn:disabled) {
  opacity: 0.5;
  cursor: not-allowed;
}

.editor-panels {
  flex: 1;
  display: flex;
  min-height: 0;
  overflow: hidden;
}

.editor-panel {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  border-right: 1px solid var(--border-color);
}

.editor-panel:last-child {
  border-right: none;
}

.panel-label {
  display: flex;
  align-items: center;
  gap: 5px;
  /* 固定 28px：与中间列 .middle-nav 同高，保证三个滚动列的内容区从同一屏幕 Y 开始，
     否则中间列内容整体下移，连接带/合并按钮与文本行垂直错位 */
  height: 28px;
  box-sizing: border-box;
  padding: 0 10px;
  font-size: 10px;
  font-weight: 600;
  color: var(--text-tertiary);
  background-color: var(--bg-toolbar);
  border-bottom: 1px solid var(--border-color);
  text-transform: uppercase;
  flex-shrink: 0;
}

.editor-container {
  flex: 1;
  display: flex;
  min-height: 0;
  overflow: hidden;
  position: relative;
  transition: box-shadow 0.15s ease;
}

/* 聚焦态：统一主题 accent 蓝 + color-mix 降透明度（描边 55% / 柔光 16%），
   用 :focus-within 在父容器画 ring —— 容器无滚动条，inset 四边都能正常显示
   （textarea 自身的 inset 会被右缘自定义滚动条 + 下缘相邻区域遮挡只露出左/上，不友好） */
.editor-container:focus-within {
  box-shadow:
    inset 0 0 0 2px color-mix(in srgb, var(--accent-primary) 55%, transparent),
    inset 0 0 0 5px color-mix(in srgb, var(--accent-primary) 16%, transparent);
}

.line-numbers {
  width: 42px;
  flex-shrink: 0;
  overflow: hidden;
  background-color: var(--bg-toolbar);
  border-right: 1px solid var(--border-color);
  /* 上 8px + 下 18px：与 textarea 内容边距一致（下边同补 10px 常驻横滚条占位），
     保证内容总高 = 行数×18+26 与 textarea scrollHeight 相等，拖到底部时最大滚动距离一致、
     行号槽不会因提前钳制而与文本错位 */
  padding: 8px 0 18px;
  box-sizing: content-box;
  text-align: right;
  -webkit-user-select: none;
  user-select: none;
}

.line-num {
  height: 18px;
  line-height: 18px;
  padding-right: 8px;
  font-size: 10px;
  color: var(--text-muted);
  font-family: Consolas, Monaco, monospace;
}

.textarea-wrapper {
  flex: 1;
  position: relative;
  min-width: 0;
}

.editor-textarea {
  width: 100%;
  height: 100%;
  resize: none;
  border: none;
  border-radius: 0;
  font-family: Consolas, Monaco, 'Courier New', monospace;
  font-size: 12px;
  line-height: 18px;
  /* 底部多留 10px：常驻横向滚动条（overflow-x:scroll）会吃掉滚动容器底部 10px 可视空间，
     但纵向 scrollHeight 默认不为它预留，导致滚到底时末行被横滚条遮挡、内容显示不全。
     这里用 padding-bottom 补偿，使末行完整浮在横滚条之上（实测 coveredPx 由 +2 变为 -8）。 */
  padding: 8px 8px 18px;
  background-color: transparent;
  /* 文字交给下层彩色渲染层显示，textarea 仅承载光标/选区/编辑 */
  color: transparent;
  caret-color: var(--text-primary);
  white-space: pre;
  overflow-y: auto;
  /* 横向滚动条常驻：避免一侧长行出现横滚条、另一侧没有，
     两侧视口高度差 10px 导致滚到底部时 scrollTop 钳制错位 */
  overflow-x: scroll;
  box-sizing: border-box;
  position: relative;
  z-index: 1;
}

/* 垂直滚动条：与查看差异面板 .side 一致（track 三级面 / thumb 边框灰 / hover 加深 / 宽10px） */
.editor-textarea::-webkit-scrollbar {
  width: 10px;
  height: 10px;
}

.editor-textarea::-webkit-scrollbar-track {
  background: var(--bg-tertiary);
}

.editor-textarea::-webkit-scrollbar-thumb {
  background: var(--border-medium);
  border-radius: 5px;
}

.editor-textarea::-webkit-scrollbar-thumb:hover {
  background: var(--text-muted);
}

/* 聚焦态指示已上移到父 .editor-container:focus-within 上做 ring（见上），
   避免 textarea 自身的 inset 阴影被右缘自定义滚动条和下缘相邻区域遮挡（只画到左/上） */
.editor-textarea:focus {
  outline: none;
}

/* 选区：蓝底 + 透明字（彩色文字层在下，选区只盖背景） */
.editor-textarea::selection {
  background-color: var(--accent-primary);
  color: transparent;
}

/* 轻量本地通知：右下角浮层，2.5 秒自动消失。 */
.local-toast {
  position: absolute;
  right: 14px;
  bottom: 14px;
  padding: 7px 12px;
  background: var(--accent-primary);
  color: #fff;
  border-radius: 6px;
  font-size: 12px;
  box-shadow: 0 6px 18px rgba(15, 23, 42, 0.18);
  pointer-events: none;
  z-index: 5;
  max-width: 70%;
  white-space: pre-wrap;
}

/* 差异着色层：铺在 textarea 之下，承载彩色文字渲染（行级底色 + 语法高亮），
   textarea 文字透明，由本层显示实际可见文本，随滚动 translate */
.diff-overlay {
  position: absolute;
  inset: 0;
  overflow: hidden;
  pointer-events: none;
  z-index: 0;
}

.diff-overlay-inner {
  padding-top: 8px;
  will-change: transform;
}

/* 每行：与 textarea 完全相同的字体度量，保证光标与彩色文字逐字符对齐 */
.hl-line {
  height: 18px;
  line-height: 18px;
  padding-left: 8px;
  white-space: pre;
  font-family: Consolas, Monaco, 'Courier New', monospace;
  font-size: 12px;
  color: var(--text-primary);
  box-sizing: border-box;
  overflow: visible;
}

/* 行级底色（与查看差异面板一致：删红·加绿·改黄，差异行整片语义底色）。
   选择器限定到 .hl-line 行级 div，避免 .hl-del 类选择器误匹配到行内 .hl-ch.hl-del span
   ——否则字符级会沿用 --bg-del/"--bg-add" 变成"文字背景色"（行/字符职责不分） */
.hl-line.hl-del {
  background-color: var(--bg-del);
}

.hl-line.hl-add {
  background-color: var(--bg-add);
}

.hl-line.hl-mod {
  background-color: var(--bg-tertiary);
}

/* 语法高亮 token 颜色（IDEA 风格，随主题切换 var(--syntax-*)） */
.tok-keyword  { color: var(--syntax-keyword); }
.tok-string   { color: var(--syntax-string); }
.tok-comment  { color: var(--syntax-comment); font-style: italic; }
.tok-number   { color: var(--syntax-number); }
.tok-function { color: var(--syntax-function); }
.tok-type     { color: var(--syntax-type); }
.tok-constant { color: var(--syntax-constant); }
.tok-plain    { color: var(--text-primary); }


/* 图例 */
.diff-legend {
  display: flex;
  align-items: center;
  gap: 12px;
  font-size: 11px;
  color: var(--text-tertiary);
  padding: 0 4px;
}

.legend-item {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  user-select: none;
}

.legend-dot {
  width: 10px;
  height: 10px;
  border-radius: 2px;
  display: inline-block;
}

.legend-dot.del {
  background-color: var(--bg-del);
  border: 1px solid var(--color-del);
}

.legend-dot.add {
  background-color: var(--bg-add);
  border: 1px solid var(--color-add);
}

.legend-dot.mod {
  background-color: var(--bg-tertiary);
  border: 1px solid var(--color-mod);
}

/* 右侧概览标尺：叠在垂直滚动条上纯展示（与查看差异面板 overview-ruler 一致） */
.diff-ruler {
  position: absolute;
  top: 0;
  right: 0;
  bottom: 0;
  width: 5px;
  background-color: transparent;
  z-index: 3;
  pointer-events: none;
  overflow: hidden;
}

.ruler-mark {
  position: absolute;
  left: 0;
  right: 0;
  pointer-events: none;
}

.ruler-del {
  background-color: var(--color-del);
}

.ruler-add {
  background-color: var(--color-add);
}

.ruler-mod {
  background-color: var(--accent-primary);
}

/* ===== 中间操作列 ===== */
.editor-middle-column {
  width: 36px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  background-color: var(--bg-toolbar);
  border-right: 1px solid var(--border-color);
  border-left: 1px solid var(--border-color);
  position: relative;
  z-index: 2;
}

.middle-nav {
  display: flex;
  flex-direction: row;
  align-items: center;
  justify-content: center;
  gap: 2px;
  /* 固定 28px：与两侧 .panel-label 同高，保证中间滚动列与文本区内容起点同 Y（IDEA 式对齐） */
  height: 28px;
  box-sizing: border-box;
  padding: 0 2px;
  background-color: var(--bg-toolbar);
  border-bottom: 1px solid var(--border-color);
  flex-shrink: 0;
}

.middle-nav :deep(button) {
  width: 18px;
  height: 18px;
  padding: 0;
}

.diff-count {
  font-size: 9px;
  font-family: Consolas, Monaco, monospace;
  color: var(--text-muted);
  user-select: none;
  padding: 1px 0;
}

.middle-actions {
  flex: 1;
  overflow-y: auto;
  overflow-x: hidden;
  position: relative;
  scrollbar-width: none;
  /* 上下各 8px：与 textarea 内容边距一致，内容总高 = 行数×18+16，
     保证最大滚动距离与 textarea 相等（否则拖到底部时连接带/合并按钮被钳制错位） */
  padding: 8px 0;
  box-sizing: border-box;
}

.middle-actions::-webkit-scrollbar {
  display: none;
  width: 0;
  height: 0;
}

/* ===== 操作行/组 ===== */
.action-group {
  height: 18px;
  line-height: 18px;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  margin: 0;
  box-sizing: border-box;
  position: relative;
}

.action-group.is-diff {
  /* 高度由 inline style 精确控制：行数 × 18px */
  padding: 0;
  margin: 0;
  position: relative;
}

/* IDEA / Beyond Compare 风格差异连接带图层：
   绝对定位于滚动内容顶部（top:0 + 高度=行数×18px），随 middle-actions 一起滚动 */
.diff-bands {
  position: absolute;
  top: 8px; /* 与 .middle-actions 的 padding-top 对齐：行 0 的带子上沿 = 文本行 0 上沿 */
  left: 0;
  width: 100%;
  z-index: 0; /* 位于操作组（含合并按钮 z-index:10）之下 */
  pointer-events: none;
}

.diff-bands .band {
  stroke-width: 1;
  stroke-linejoin: round;
  stroke-opacity: 0.45; /* 描边降不透明度，视觉更柔和 */
}

.diff-bands .band-del {
  fill: rgba(214, 48, 49, 0.09);
  stroke: var(--color-del);
}

.diff-bands .band-add {
  fill: rgba(46, 160, 67, 0.09);
  stroke: var(--color-add);
}

.diff-bands .band-mod {
  fill: rgba(188, 129, 20, 0.09);
  stroke: var(--color-mod);
}

.mid-placeholder {
  display: block;
  width: 100%;
}

/* 差异组按钮容器：绝对定位在组中心 */
.merge-buttons {
  position: absolute;
  left: 50%;
  transform: translate(-50%, -50%);
  display: flex;
  align-items: center;
  gap: 2px;
  z-index: 10;
  background-color: var(--bg-toolbar);
  border-radius: 4px;
  padding: 2px;
  box-shadow: 0 1px 4px rgba(0, 0, 0, 0.15);
}

/* ===== VS Code 风格合并按钮 ===== */
.merge-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  border: none;
  border-radius: 3px;
  cursor: pointer;
  padding: 0;
  color: var(--text-tertiary);
  background-color: transparent;
  transition: all 0.12s ease;
  position: relative;
}

.merge-btn .arrow-icon {
  opacity: 0.6;
  transition: opacity 0.12s ease;
}

.merge-btn:hover {
  background-color: var(--bg-hover);
}

.merge-btn:hover .arrow-icon {
  opacity: 1;
}

/* 接受左侧 → 绿色（接受 HEAD 版本） */
.merge-btn.accept-left {
  color: var(--color-add, #2ea043);
}

.merge-btn.accept-left:hover {
  background-color: rgba(46, 160, 67, 0.2);
}

.merge-btn.accept-left:hover .arrow-icon {
  opacity: 1;
  filter: drop-shadow(0 0 3px rgba(46, 160, 67, 0.6));
}

/* 接受右侧 → 蓝色（接受工作区版本） */
.merge-btn.accept-right {
  color: var(--accent-primary, #388bfd);
}

.merge-btn.accept-right:hover {
  background-color: rgba(56, 139, 253, 0.2);
}

.merge-btn.accept-right:hover .arrow-icon {
  opacity: 1;
  filter: drop-shadow(0 0 3px rgba(56, 139, 253, 0.6));
}

/* del 行组：接受按钮偏红 */
.action-group.group-del .merge-btn.accept-left {
  color: #d73a49;
}

.action-group.group-del .merge-btn.accept-right {
  color: #d73a49;
}

.action-group.group-del .merge-btn:hover {
  background-color: rgba(215, 58, 73, 0.2);
}

/* add 行组：接受按钮偏绿 */
.action-group.group-add .merge-btn.accept-left {
  color: #2ea043;
}

.action-group.group-add .merge-btn.accept-right {
  color: #2ea043;
}

.action-group.group-add .merge-btn:hover {
  background-color: rgba(46, 160, 67, 0.2);
}

/* mod 行组：接受按钮偏黄/橙 */
.action-group.group-mod .merge-btn.accept-left {
  color: #bf8700;
}

.action-group.group-mod .merge-btn.accept-right {
  color: #bf8700;
}

.action-group.group-mod .merge-btn:hover {
  background-color: rgba(191, 135, 0, 0.2);
}

.merge-btn:active {
  transform: scale(0.9);
}
</style>


