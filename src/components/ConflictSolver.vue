<script setup lang="ts">
import { ref, computed, watch, nextTick, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import type { ConflictFile, ConflictBlock } from '../types'
import { fileLang } from '../syntaxHighlight'
import { diffWords, plainSegments, changedSegments, mergeSegments, type Segment } from '../wordDiff'
import {
  ChevronUp, ChevronDown, ArrowLeftToLine, ArrowRightToLine,
  AlignJustify, Save, X, GitMerge, ArrowLeft, ArrowRight,
  Combine, Rows3,
} from 'lucide-vue-next'
import { Button } from '@/components/ui/button'
import { writeFileContent, stageFile } from '../api/git'

interface Props {
  conflictFile: ConflictFile
  repoPath: string
}

const props = defineProps<Props>()
const emit = defineEmits(['save', 'close'])
const { t } = useI18n()

const workingContent = ref(props.conflictFile.working_content.join('\n'))
const currentBlockIndex = ref(0)
const isSaving = ref(false)

const leftTextarea = ref<HTMLTextAreaElement | null>(null)
const middleTextarea = ref<HTMLTextAreaElement | null>(null)
const rightTextarea = ref<HTMLTextAreaElement | null>(null)
const leftLinesRef = ref<HTMLElement | null>(null)
const middleLinesRef = ref<HTMLElement | null>(null)
const rightLinesRef = ref<HTMLElement | null>(null)
// 着色层（语法高亮）与两列之间的操作列，随文本滚动同步
const leftOverlayInner = ref<HTMLElement | null>(null)
const middleOverlayInner = ref<HTMLElement | null>(null)
const rightOverlayInner = ref<HTMLElement | null>(null)
const leftMidColRef = ref<HTMLElement | null>(null)
const rightMidColRef = ref<HTMLElement | null>(null)
// 中间列顶部占位高度 = 相邻文本栏 .panel-label 高度（运行时实测），用于把弧形/箭头按钮的
// 垂直坐标原点对齐到编辑器内容原点（文本栏被 panel-label 整体下压了同样高度，中间列没有，会偏高 ≈L）
const midcolTopPad = ref(28)

const LINE_HEIGHT = 18
const PADDING_TOP = 8

watch(
  () => props.conflictFile,
  (cf) => {
    workingContent.value = cf.working_content.join('\n')
    currentBlockIndex.value = 0
  }
)

function splitLinesKeepNewline(content: string): string[] {
  const lines = content.split('\n')
  return lines.map((line, index) => (index < lines.length - 1 ? line + '\n' : line))
}

function parseConflictBlocks(content: string): ConflictBlock[] {
  const lines = splitLinesKeepNewline(content)
  const result: ConflictBlock[] = []
  let i = 0

  while (i < lines.length) {
    if (lines[i].startsWith('<<<<<<< ')) {
      const startLine = i
      const markerOurs = lines[i].slice(8).trimEnd()
      i++

      const ours: string[] = []
      let base: string[] | undefined
      const theirs: string[] = []
      let separatorLine = i
      let section = 0

      while (i < lines.length) {
        const cur = lines[i]
        if (cur.startsWith('||||||| ')) {
          base = []
          section = 1
          i++
        } else if (cur.startsWith('=======')) {
          separatorLine = i
          section = 2
          i++
        } else if (cur.startsWith('>>>>>>> ')) {
          const markerTheirs = cur.slice(8).trimEnd()
          result.push({
            start_line: startLine,
            separator_line: separatorLine,
            end_line: i,
            ours,
            theirs,
            base,
            marker_ours: markerOurs,
            marker_theirs: markerTheirs,
          })
          i++
          break
        } else {
          if (section === 0) ours.push(cur)
          else if (section === 1 && base) base.push(cur)
          else if (section === 2) theirs.push(cur)
          i++
        }
      }
    } else {
      i++
    }
  }

  return result
}

const blocks = computed(() => parseConflictBlocks(workingContent.value))
const hasBlocks = computed(() => blocks.value.length > 0)
const currentBlock = computed(() => blocks.value[currentBlockIndex.value] ?? null)

const oursText = computed(() => props.conflictFile.ours_content.join('\n'))
const theirsText = computed(() => props.conflictFile.theirs_content.join('\n'))

// 语言（决定语法高亮规则）
const lang = computed(() => fileLang(props.conflictFile.path))

/** 去掉行尾换行：冲突块片段来自 splitLinesKeepNewline（带 '\n'），而 ours_content/theirs_content 不带 */
function stripEol(s: string): string {
  return s.replace(/\r?\n$/, '')
}

// ---- 行内（词级）差异：参考 IDEA / SmartGit 的合并编辑器 ----
// 每个冲突块内把 ours / theirs 按行对齐，逐行做 token 级 LCS 差异，标出「只属于某一侧」的词。
// 与行级底色叠加，形成「行级色块 + 词级高亮」两层差异表达；这在原先只有整行色块的基础上，
// 让用户直接看到「这一行里到底哪几个词不同」。
interface BlockLineSegs {
  left: Segment[]
  right: Segment[]
}

const blockWordSegs = computed<BlockLineSegs[][]>(() =>
  blocks.value.map((b) => {
    const n = Math.max(b.ours.length, b.theirs.length)
    const rows: BlockLineSegs[] = []
    for (let i = 0; i < n; i++) {
      const hasL = i < b.ours.length
      const hasR = i < b.theirs.length
      const l = hasL ? stripEol(b.ours[i]) : ''
      const r = hasR ? stripEol(b.theirs[i]) : ''
      if (hasL && hasR) {
        const d = diffWords(l, r)
        rows.push({ left: mergeSegments(d.left, l, lang.value), right: mergeSegments(d.right, r, lang.value) })
      } else if (hasL) {
        rows.push({ left: changedSegments(l, lang.value), right: [] })
      } else {
        rows.push({ left: [], right: changedSegments(r, lang.value) })
      }
    }
    return rows
  })
)

// ---- 冲突块片段在 ours / theirs 全文中的行范围（顺序查找）----
// 用于在左右两栏「仅标记冲突片段所在的行」（非整栏染色），与 working 栏的差异标记对应。
interface FragSpan {
  first: number
  last: number
}

function findFragment(full: string[], frag: string[], from: number): number {
  if (frag.length === 0) return -1
  outer: for (let s = from; s + frag.length <= full.length; s++) {
    for (let k = 0; k < frag.length; k++) {
      if (full[s + k] !== frag[k]) continue outer
    }
    return s
  }
  return -1
}

function locateFragments(full: string[], pick: (b: ConflictBlock) => string[]): (FragSpan | null)[] {
  let pos = 0
  return blocks.value.map((b) => {
    const frag = pick(b).map(stripEol)
    const idx = findFragment(full, frag, pos)
    if (idx < 0) return null
    pos = idx + frag.length
    return { first: idx, last: idx + frag.length - 1 }
  })
}

const oursFragSpans = computed(() => locateFragments(props.conflictFile.ours_content, (b) => b.ours))
const theirsFragSpans = computed(() => locateFragments(props.conflictFile.theirs_content, (b) => b.theirs))

// 逐行渲染模型：一行 = 词级分段 + 行级 class
interface LineModel {
  segs: Segment[]
  cls: string
}

function buildSideLines(
  full: string[],
  spans: (FragSpan | null)[],
  pick: (b: ConflictBlock) => string[],
  side: 'ours' | 'theirs',
): LineModel[] {
  const models: LineModel[] = full.map((ln) => ({ segs: plainSegments(ln, lang.value), cls: '' }))
  blocks.value.forEach((b, bi) => {
    const span = spans[bi]
    if (!span) return
    const cur = bi === currentBlockIndex.value ? ' current' : ''
    const fragLen = pick(b).length
    for (let k = 0; k < fragLen; k++) {
      const li = span.first + k
      if (li >= models.length) break
      const row = blockWordSegs.value[bi]?.[k]
      const segs = (side === 'ours' ? row?.left : row?.right) ?? []
      models[li] = { segs, cls: `side-${side}${cur}` }
    }
  })
  return models
}

const oursLines = computed(() =>
  buildSideLines(props.conflictFile.ours_content, oursFragSpans.value, (b) => b.ours, 'ours'),
)
const theirsLines = computed(() =>
  buildSideLines(props.conflictFile.theirs_content, theirsFragSpans.value, (b) => b.theirs, 'theirs'),
)

// 中间（working）栏：逐行「词级分段 + 行角色 class」。
// 行级底色与编辑页 .hl-del/.hl-add 同源：ours 段红 / theirs 段绿 / 标记行灰 / base（diff3 祖先）行灰；
// 当前块追加 current 强调。
const middleLines = computed<LineModel[]>(() => {
  const lines = workingContent.value.split('\n')
  const models: LineModel[] = lines.map((ln) => ({ segs: plainSegments(ln, lang.value), cls: '' }))
  const mark = (idx: number, cls: string, segs?: Segment[]) => {
    if (idx < 0 || idx >= models.length) return
    const merged = models[idx].cls ? models[idx].cls + ' ' + cls : cls
    models[idx] = { segs: segs ?? models[idx].segs, cls: merged }
  }
  blocks.value.forEach((b, bi) => {
    const cur = bi === currentBlockIndex.value ? ' current' : ''
    const wordRows = blockWordSegs.value[bi] ?? []
    // ours 段
    for (let k = 0; k < b.ours.length; k++) {
      mark(b.start_line + 1 + k, `line-confl-ours${cur}`, wordRows[k]?.left)
    }
    // base 段（仅在 merge.conflictStyle=diff3 时存在）
    if (b.base && b.base.length > 0) {
      const baseStart = b.separator_line - b.base.length
      for (let k = 0; k < b.base.length; k++) mark(baseStart + k, `line-confl-base${cur}`)
      mark(baseStart - 1, `line-confl-marker${cur}`)
    }
    // theirs 段
    for (let k = 0; k < b.theirs.length; k++) {
      mark(b.end_line - b.theirs.length + k, `line-confl-theirs${cur}`, wordRows[k]?.right)
    }
    // 三种标记行
    mark(b.start_line, `line-confl-marker${cur}`)
    mark(b.separator_line, `line-confl-marker${cur}`)
    mark(b.end_line, `line-confl-marker${cur}`)
  })
  return models
})

const allLineCount = computed(() => workingContent.value.split('\n').length)
const leftLineCount = computed(() => props.conflictFile.ours_content.length || 1)
const rightLineCount = computed(() => props.conflictFile.theirs_content.length || 1)

// 当前冲突块在左/右操作列中的垂直定位（与编辑区行号对应）
function blockTop(idx: number): number {
  const b = blocks.value[idx]
  return b ? PADDING_TOP + b.start_line * LINE_HEIGHT : 0
}
function blockHeight(idx: number): number {
  const b = blocks.value[idx]
  return b ? (b.end_line - b.start_line + 1) * LINE_HEIGHT : 0
}

// ---- 中间列弧形背景色标记（与「编辑页面」diff-bands 同构的 S 曲线漏斗） ----
// 编辑页有 rowModel 行对齐模型，可算出 hunk 左右两侧各自的「有内容行范围」，再用三次贝塞尔
// （S 曲线）把两侧的上/下沿连起来：两侧行数不同 → 梯形/漏斗；某一侧无内容 → 该侧塌缩成块的
// 纵向中点（叶子形）。
// 冲突页三栏是纯 textarea、没有行对齐模型，故两侧端点统一取 working 文档坐标，避免出现与文本
// 错位的长斜条（三栏同步滚动、同为 18px 行高，坐标系可直接对应）：
//   · 左中列（ours ↔ working）：左沿 = 该块 ours 内容在块内的行范围，右沿 = 整个冲突块的范围
//   · 右中列（working ↔ theirs）：左沿 = 整个冲突块的范围，右沿 = 该块 theirs 内容在块内的行范围
// 某侧内容为空（纯增 / 纯删）时该侧塌缩成一个点 → 即编辑页截图里的叶子形。
interface DocSpan {
  first: number
  last: number
}

/** 冲突块某一侧内容在块内的行范围（working 文档坐标，已排除 <<<<<<< / ||||||| / ======= / >>>>>>> 标记行）；该侧为空时返回 null */
function sideSpanInBlock(b: ConflictBlock, side: 'ours' | 'theirs'): DocSpan | null {
  const first = side === 'ours' ? b.start_line + 1 : b.end_line - b.theirs.length
  const last = side === 'ours' ? b.start_line + b.ours.length : b.end_line - 1
  return last >= first ? { first, last } : null
}

const MIDCOL_W = 35 // .solver-midcol 内容宽（36px 减 1px border-right）
const BAND_INSET = 0.75 // 左右各内缩，避免描边被容器裁掉

/** 与编辑页 hunks 完全同构的路径：上下沿各一条 S 曲线把两栏连起来 */
function buildBandPath(left: DocSpan | null, right: DocSpan | null, midY: number): string {
  const x0 = BAND_INSET
  const x1 = MIDCOL_W - BAND_INSET
  const c = MIDCOL_W / 2
  const lt = left ? PADDING_TOP + left.first * LINE_HEIGHT : midY
  const lb = left ? PADDING_TOP + (left.last + 1) * LINE_HEIGHT : midY
  const rt = right ? PADDING_TOP + right.first * LINE_HEIGHT : midY
  const rb = right ? PADDING_TOP + (right.last + 1) * LINE_HEIGHT : midY
  return [
    `M ${x0} ${lt}`,
    `C ${c} ${lt} ${c} ${rt} ${x1} ${rt}`,
    `L ${x1} ${rb}`,
    `C ${c} ${rb} ${c} ${lb} ${x0} ${lb}`,
    'Z',
  ].join(' ')
}

/** 冲突块纵向中点：某一侧无内容时该侧塌缩到此（与编辑页 mid 语义一致） */
function blockMidY(idx: number): number {
  const b = blocks.value[idx]
  if (!b) return PADDING_TOP
  return PADDING_TOP + ((b.start_line + b.end_line + 1) / 2) * LINE_HEIGHT
}

const bandsTotalHeight = computed(() => allLineCount.value * LINE_HEIGHT + PADDING_TOP)

// 左中列：绿（ours）——左沿接该块 ours 内容的行范围，右沿接整个冲突块的行范围
const leftBands = computed(() =>
  blocks.value.map((b, idx) => ({
    path: buildBandPath(
      sideSpanInBlock(b, 'ours'),
      { first: b.start_line, last: b.end_line },
      blockMidY(idx)
    ),
    active: idx === currentBlockIndex.value,
  }))
)

// 右中列：蓝（theirs）——左沿接整个冲突块的行范围，右沿接该块 theirs 内容的行范围
const rightBands = computed(() =>
  blocks.value.map((b, idx) => ({
    path: buildBandPath(
      { first: b.start_line, last: b.end_line },
      sideSpanInBlock(b, 'theirs'),
      blockMidY(idx)
    ),
    active: idx === currentBlockIndex.value,
  }))
)

watch(currentBlock, async () => {
  await nextTick()
  scrollToCurrentBlock()
})

onMounted(() => {
  measureLabelHeight()
  scrollToCurrentBlock()
})

function scrollToCurrentBlock() {
  const block = currentBlock.value
  const ta = middleTextarea.value
  if (!block || !ta) return
  const targetTop = PADDING_TOP + block.start_line * LINE_HEIGHT
  ta.scrollTop = Math.max(0, targetTop - ta.clientHeight / 3)
}

// 实测相邻文本栏 .panel-label 的高度，作为中间列顶部占位，使弧形/箭头按钮与编辑器内容原点
// （被 panel-label 下压了同样高度）精确对齐。主题/CSS 改动后自动跟随，无需硬编码。
function measureLabelHeight() {
  const lbl = document.querySelector<HTMLElement>('.solver-panels .panel-label')
  if (lbl) midcolTopPad.value = lbl.offsetHeight
}

function syncScroll(source: 'left' | 'middle' | 'right') {
  const refs: Record<string, HTMLTextAreaElement | null> = {
    left: leftTextarea.value,
    middle: middleTextarea.value,
    right: rightTextarea.value,
  }
  const lineRefs: Record<string, HTMLElement | null> = {
    left: leftLinesRef.value,
    middle: middleLinesRef.value,
    right: rightLinesRef.value,
  }
  const overlayInners: Record<string, HTMLElement | null> = {
    left: leftOverlayInner.value,
    middle: middleOverlayInner.value,
    right: rightOverlayInner.value,
  }
  const sourceEl = refs[source]
  if (!sourceEl) return
  const scrollTop = sourceEl.scrollTop
  const scrollLeft = sourceEl.scrollLeft

  ;(['left', 'middle', 'right'] as const).forEach((key) => {
    const el = refs[key]
    const lineEl = lineRefs[key]
    const ov = overlayInners[key]
    if (el && el !== sourceEl) {
      el.scrollTop = scrollTop
      el.scrollLeft = scrollLeft
    }
    if (lineEl) lineEl.scrollTop = scrollTop
    // 着色层跟随滚动（translate 不触发 scroll 事件，安全）
    if (ov) ov.style.transform = `translate(${-scrollLeft}px, ${-scrollTop}px)`
  })

  // 两侧操作列跟随编辑区滚动：按钮按冲突块行号定位，需与文本保持垂直对齐
  if (leftMidColRef.value) leftMidColRef.value.scrollTop = scrollTop
  if (rightMidColRef.value) rightMidColRef.value.scrollTop = scrollTop
}

function goToPreviousBlock() {
  const list = blocks.value
  if (list.length === 0) return
  currentBlockIndex.value = (currentBlockIndex.value - 1 + list.length) % list.length
}

function goToNextBlock() {
  const list = blocks.value
  if (list.length === 0) return
  currentBlockIndex.value = (currentBlockIndex.value + 1) % list.length
}

function applyCurrentBlock(side: 'ours' | 'theirs' | 'combine' | 'combine-reverse') {
  applyBlock(currentBlockIndex.value, side)
}

function applyAllBlocks(side: 'ours' | 'theirs' | 'combine' | 'combine-reverse') {
  const list = blocks.value
  for (let i = list.length - 1; i >= 0; i--) {
    applyBlock(i, side)
  }
}

function applyBlock(index: number, side: 'ours' | 'theirs' | 'combine' | 'combine-reverse') {
  const list = parseConflictBlocks(workingContent.value)
  const block = list[index]
  if (!block) return

  let replacement = ''
  switch (side) {
    case 'ours':
      replacement = block.ours.join('')
      break
    case 'theirs':
      replacement = block.theirs.join('')
      break
    case 'combine':
      replacement = block.ours.join('') + block.theirs.join('')
      break
    case 'combine-reverse':
      replacement = block.theirs.join('') + block.ours.join('')
      break
  }

  const lines = splitLinesKeepNewline(workingContent.value)
  workingContent.value = [
    ...lines.slice(0, block.start_line),
    replacement,
    ...lines.slice(block.end_line + 1),
  ].join('')

  const newList = parseConflictBlocks(workingContent.value)
  if (newList.length === 0) {
    currentBlockIndex.value = 0
  } else if (index >= newList.length) {
    currentBlockIndex.value = newList.length - 1
  }
}

async function handleSave() {
  if (isSaving.value) return
  isSaving.value = true
  try {
    await writeFileContent(props.repoPath, props.conflictFile.path, workingContent.value)
    // 解决冲突 = 写回内容后 git add 标记冲突已解决，刷新后该文件不再列为冲突
    await stageFile(props.repoPath, props.conflictFile.path)
    emit('save')
  } catch (e) {
    console.error('Save conflict file error:', e)
    alert(t('conflictSolver.saveFailed', { error: (typeof e === 'string' ? e : (e as any)?.toString?.() || String(e)) }))
  } finally {
    isSaving.value = false
  }
}

function getFileName(path: string): string {
  const parts = path.replace(/\\/g, '/').split('/')
  return parts[parts.length - 1] || path
}

function lineNumberArray(count: number): number[] {
  return Array.from({ length: count }, (_, i) => i + 1)
}
</script>

<template>
  <div class="conflict-solver">
    <!-- 顶部工具栏 -->
    <div class="solver-toolbar">
      <div class="solver-title">
        <GitMerge :size="14" class="title-icon" />
        <span>{{ t('conflictSolver.title') }}</span>
        <span class="file-name">{{ getFileName(conflictFile.path) }}</span>
      </div>
      <div class="solver-actions">
        <Button variant="outline" size="sm" @click="goToPreviousBlock" :disabled="!hasBlocks" :title="t('conflictSolver.prevBlockTitle')">
          <ChevronUp :size="14" />
          {{ t('conflictSolver.prevBlock') }}
        </Button>
        <Button variant="outline" size="sm" @click="goToNextBlock" :disabled="!hasBlocks" :title="t('conflictSolver.nextBlockTitle')">
          <ChevronDown :size="14" />
          {{ t('conflictSolver.nextBlock') }}
        </Button>

        <div class="toolbar-divider"></div>

        <Button variant="outline" size="sm" @click="applyCurrentBlock('ours')" :disabled="!currentBlock" :title="t('conflictSolver.useLeftTitle')">
          <ArrowLeftToLine :size="14" />
          {{ t('conflictSolver.useLeft') }}
        </Button>
        <Button variant="outline" size="sm" @click="applyCurrentBlock('theirs')" :disabled="!currentBlock" :title="t('conflictSolver.useRightTitle')">
          <ArrowRightToLine :size="14" />
          {{ t('conflictSolver.useRight') }}
        </Button>

        <div class="toolbar-divider"></div>

        <Button variant="outline" size="sm" @click="applyAllBlocks('ours')" :disabled="!hasBlocks" :title="t('conflictSolver.allLeftTitle')">
          <AlignJustify :size="14" />
          {{ t('conflictSolver.allLeft') }}
        </Button>
        <Button variant="outline" size="sm" @click="applyAllBlocks('theirs')" :disabled="!hasBlocks" :title="t('conflictSolver.allRightTitle')">
          <AlignJustify :size="14" />
          {{ t('conflictSolver.allRight') }}
        </Button>
        <Button variant="outline" size="sm" @click="applyAllBlocks('combine')" :disabled="!hasBlocks" :title="t('conflictSolver.allCombineTitle')">
          <Combine :size="14" />
          {{ t('conflictSolver.allCombine') }}
        </Button>

        <div class="toolbar-divider"></div>

        <Button size="sm" @click="handleSave" :disabled="isSaving">
          <Save :size="14" />
          {{ isSaving ? t('conflictSolver.saving') : t('conflictSolver.save') }}
        </Button>
        <Button variant="ghost" size="sm" @click="emit('close')">
          <X :size="14" />
        </Button>
      </div>
    </div>

    <!-- 三栏编辑区 -->
    <div class="solver-panels">
      <!-- ours -->
      <div class="solver-panel">
        <div class="panel-label">
          <ArrowLeft :size="11" />
          <span>{{ t('conflictSolver.oursLabel') }}</span>
        </div>
        <div class="editor-container">
          <div ref="leftLinesRef" class="line-numbers">
            <div v-for="n in lineNumberArray(leftLineCount)" :key="n" class="line-num">{{ n }}</div>
          </div>
          <div class="textarea-wrapper">
            <div class="diff-overlay" aria-hidden="true">
              <div ref="leftOverlayInner" class="diff-overlay-inner">
                <div v-for="(line, idx) in oursLines" :key="'lo' + idx" class="hl-line" :class="line.cls"><span v-for="(seg, si) in line.segs" :key="si" :class="['tok-' + seg.type, { 'seg-chg': seg.changed }]">{{ seg.text }}</span></div>
              </div>
            </div>
            <textarea
              ref="leftTextarea"
              :value="oursText"
              readonly
              class="solver-textarea"
              spellcheck="false"
              @scroll="syncScroll('left')"
            />
          </div>
        </div>
      </div>

      <!-- 左中间操作列：ours ↔ working，每个冲突块对应位置一个「接受 ours」箭头 -->
      <div class="solver-midcol" ref="leftMidColRef">
        <div class="midcol-label-spacer" :style="{ height: midcolTopPad + 'px' }"></div>
        <div class="midcol-inner" :style="{ minHeight: bandsTotalHeight + 'px' }">
          <!-- 弧形背景色标记：每个冲突块一段 S 曲线漏斗，绿=ours，当前块高亮 -->
          <svg
            class="diff-bands band-ours"
            :viewBox="`0 0 ${MIDCOL_W} ${bandsTotalHeight}`"
            :style="{ height: bandsTotalHeight + 'px' }"
            preserveAspectRatio="none"
            aria-hidden="true"
          >
            <path
              v-for="(b, bi) in leftBands"
              :key="'lb' + bi"
              class="band"
              :class="{ active: b.active }"
              :d="b.path"
              vector-effect="non-scaling-stroke"
            />
          </svg>
          <div
            v-for="(_, bi) in blocks"
            :key="'lm' + bi"
            class="solver-action-group"
            :style="{ top: blockTop(bi) + 'px', height: blockHeight(bi) + 'px' }"
          >
            <button class="merge-btn accept-left" :title="t('conflictSolver.acceptOursTitle')" @click="applyBlock(bi, 'ours')">
              <svg class="arrow-icon" viewBox="0 0 16 16" fill="currentColor" width="14" height="14"><path d="M10 3L5 8l5 5V3z" transform="rotate(180 8 8)"/></svg>
            </button>
          </div>
        </div>
      </div>

      <!-- working tree -->
      <div class="solver-panel middle-panel">
        <div class="panel-label">
          <Rows3 :size="11" />
          <span>{{ t('conflictSolver.workingLabel') }}</span>
        </div>
        <div class="editor-container">
          <div ref="middleLinesRef" class="line-numbers">
            <div v-for="n in lineNumberArray(allLineCount)" :key="n" class="line-num">{{ n }}</div>
          </div>
          <div class="textarea-wrapper">
            <div class="diff-overlay" aria-hidden="true">
              <div ref="middleOverlayInner" class="diff-overlay-inner">
                <div v-for="(line, idx) in middleLines" :key="'mo' + idx" class="hl-line" :class="line.cls"><span v-for="(seg, si) in line.segs" :key="si" :class="['tok-' + seg.type, { 'seg-chg': seg.changed }]">{{ seg.text }}</span></div>
              </div>
            </div>
            <textarea
              ref="middleTextarea"
              v-model="workingContent"
              class="solver-textarea"
              spellcheck="false"
              @scroll="syncScroll('middle')"
            />
          </div>
        </div>

        <!-- 当前冲突块的 inline 操作小部件 -->
        <div v-if="currentBlock" class="inline-widget">
          <span class="widget-label">{{ t('conflictSolver.conflictWidget', { current: currentBlockIndex + 1, total: blocks.length }) }}</span>
          <Button variant="outline" size="sm" @click="applyCurrentBlock('ours')">
            <ArrowLeft :size="12" />
            {{ t('conflictSolver.left') }}
          </Button>
          <Button variant="outline" size="sm" @click="applyCurrentBlock('theirs')">
            {{ t('conflictSolver.right') }}
            <ArrowRight :size="12" />
          </Button>
          <Button variant="outline" size="sm" @click="applyCurrentBlock('combine')">
            <Combine :size="12" />
            {{ t('conflictSolver.combine') }}
          </Button>
        </div>
      </div>

      <!-- 右中间操作列：working ↔ theirs，每个冲突块对应位置一个「接受 theirs」箭头 -->
      <div class="solver-midcol" ref="rightMidColRef">
        <div class="midcol-label-spacer" :style="{ height: midcolTopPad + 'px' }"></div>
        <div class="midcol-inner" :style="{ minHeight: bandsTotalHeight + 'px' }">
          <!-- 弧形背景色标记：每个冲突块一段 S 曲线漏斗，蓝=theirs，当前块高亮 -->
          <svg
            class="diff-bands band-theirs"
            :viewBox="`0 0 ${MIDCOL_W} ${bandsTotalHeight}`"
            :style="{ height: bandsTotalHeight + 'px' }"
            preserveAspectRatio="none"
            aria-hidden="true"
          >
            <path
              v-for="(b, bi) in rightBands"
              :key="'rb' + bi"
              class="band"
              :class="{ active: b.active }"
              :d="b.path"
              vector-effect="non-scaling-stroke"
            />
          </svg>
          <div
            v-for="(_, bi) in blocks"
            :key="'rm' + bi"
            class="solver-action-group"
            :style="{ top: blockTop(bi) + 'px', height: blockHeight(bi) + 'px' }"
          >
            <button class="merge-btn accept-right" :title="t('conflictSolver.acceptTheirsTitle')" @click="applyBlock(bi, 'theirs')">
              <svg class="arrow-icon" viewBox="0 0 16 16" fill="currentColor" width="14" height="14"><path d="M10 3L5 8l5 5V3z"/></svg>
            </button>
          </div>
        </div>
      </div>

      <!-- theirs -->
      <div class="solver-panel">
        <div class="panel-label">
          <span>{{ t('conflictSolver.theirsLabel') }}</span>
          <ArrowRight :size="11" />
        </div>
        <div class="editor-container">
          <div ref="rightLinesRef" class="line-numbers">
            <div v-for="n in lineNumberArray(rightLineCount)" :key="n" class="line-num">{{ n }}</div>
          </div>
          <div class="textarea-wrapper">
            <div class="diff-overlay" aria-hidden="true">
              <div ref="rightOverlayInner" class="diff-overlay-inner">
                <div v-for="(line, idx) in theirsLines" :key="'ro' + idx" class="hl-line" :class="line.cls"><span v-for="(seg, si) in line.segs" :key="si" :class="['tok-' + seg.type, { 'seg-chg': seg.changed }]">{{ seg.text }}</span></div>
              </div>
            </div>
            <textarea
              ref="rightTextarea"
              :value="theirsText"
              readonly
              class="solver-textarea"
              spellcheck="false"
              @scroll="syncScroll('right')"
            />
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.conflict-solver {
  display: flex;
  flex-direction: column;
  height: 100%;
  background-color: var(--bg-secondary);
  min-height: 0;
}

.solver-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 6px 10px;
  background-color: var(--bg-toolbar);
  border-bottom: 1px solid var(--border-color);
  flex-shrink: 0;
}

.solver-title {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  font-weight: 600;
  color: var(--text-secondary);
}

.title-icon {
  color: var(--accent-text);
}

.file-name {
  font-family: Consolas, Monaco, monospace;
  color: var(--accent-text);
  font-weight: 500;
}

.solver-actions {
  display: flex;
  align-items: center;
  gap: 6px;
}

.toolbar-divider {
  width: 1px;
  height: 18px;
  background-color: var(--border-color);
  margin: 0 2px;
}

.solver-actions :deep(button) {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  font-size: 11px;
  padding: 0 10px;
  height: 26px;
}

.solver-panels {
  flex: 1;
  display: flex;
  min-height: 0;
  overflow: hidden;
}

.solver-panel {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  border-right: 1px solid var(--border-color);
  position: relative;
}

.solver-panel:last-child {
  border-right: none;
}

.middle-panel {
  flex: 1.25;
}

.panel-label {
  display: flex;
  align-items: center;
  gap: 5px;
  padding: 5px 10px;
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
}

.line-numbers {
  width: 42px;
  flex-shrink: 0;
  overflow: hidden;
  background-color: var(--bg-toolbar);
  border-right: 1px solid var(--border-color);
  padding-top: 8px;
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
  /* 背景交给 wrapper：textarea 透明（文字由下方着色层显示），背景须在此层兜底 */
  background-color: var(--bg-tertiary);
}

.middle-panel .textarea-wrapper {
  background-color: var(--bg-primary);
}

.solver-textarea {
  width: 100%;
  height: 100%;
  resize: none;
  border: none;
  border-radius: 0;
  font-family: Consolas, Monaco, 'Courier New', monospace;
  font-size: 12px;
  line-height: 18px;
  /* 底部 18px：常驻横向滚动条占位，使 scrollHeight 与着色层逐行对齐，滚到底不钳制 */
  padding: 8px 8px 18px;
  /* 文字透明，实际可见文本由下方着色层（.diff-overlay）渲染 */
  background-color: transparent;
  color: transparent;
  caret-color: var(--text-primary);
  white-space: pre;
  overflow: auto;
  box-sizing: border-box;
  position: relative;
  z-index: 1;
}

/* 聚焦态：与编辑页一致——只在「编辑区顶部」画 2px accent 线 + 3px 柔光，不留四边框。
   原先挂在 textarea 自身的 inset ring 会四面全露（截图实测 x505..993 / y40..918 一整圈）。
   编辑页 .editor-container:focus-within 的 ring 名义上也是四边，但左被行号槽、右/下被
   textarea 自己的不透明滚动条盖住，可见部分只有顶边（实测左带/右带蓝像素均为 0）。
   这里用 wrapper 伪元素复刻同一可见结果：z-index:0 置于 textarea(z-index:1) 之下，
   于是右端被竖直滚动条截断、左端自然从行号槽右侧起。2px@55% + 3px@16% 与编辑页规格一致。 */
.solver-textarea:focus {
  outline: none;
}

.textarea-wrapper::before,
.textarea-wrapper::after {
  content: '';
  position: absolute;
  left: 0;
  right: 0;
  z-index: 0;
  opacity: 0;
  pointer-events: none;
  transition: opacity 0.15s ease;
}

.textarea-wrapper::before {
  top: 0;
  height: 2px;
  background-color: color-mix(in srgb, var(--accent-primary) 55%, transparent);
}

.textarea-wrapper::after {
  top: 2px;
  height: 3px;
  background-color: color-mix(in srgb, var(--accent-primary) 16%, transparent);
}

.textarea-wrapper:focus-within::before,
.textarea-wrapper:focus-within::after {
  opacity: 1;
}

/* 选区：半透明蓝底 + 透明字（同编辑页）。
   选区背景画在 textarea（z-index:1）之上、彩色文字在 .diff-overlay（z-index:0）之下，
   实色 --accent-primary 会把文字整块盖住 → 改用半透明 --bg-selection 透出文字。 */
.solver-textarea::selection {
  background-color: var(--bg-selection);
  color: transparent;
}

/* 着色层：铺在 textarea 之下，承载语法高亮文字；textarea 透明，由本层显示可见文本 */
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

/* 语法高亮 token 颜色（IDEA 风格，随主题切换 var(--syntax-*)） */
.tok-keyword  { color: var(--syntax-keyword); }
.tok-string   { color: var(--syntax-string); }
.tok-comment  { color: var(--syntax-comment); font-style: italic; }
.tok-number   { color: var(--syntax-number); }
.tok-function { color: var(--syntax-function); }
.tok-type     { color: var(--syntax-type); }
.tok-constant { color: var(--syntax-constant); }
.tok-plain    { color: var(--text-primary); }

/* 行级底色（对齐编辑页 .hl-line.hl-del/.hl-add/.hl-mod）：整行底色、无边框。
   标记行灰 / base（diff3 祖先）灰、ours 段红（--bg-del）、theirs 段绿（--bg-add）。 */
.hl-line.line-confl-marker,
.hl-line.line-confl-base {
  background-color: var(--bg-tertiary);
}

.hl-line.line-confl-ours {
  background-color: var(--bg-del);
}

.hl-line.line-confl-theirs {
  background-color: var(--bg-add);
}

/* 左右两栏：仅在该冲突块「片段所在的行」上底色（不是整栏染色），
   标出这一侧参与冲突的区域 —— 与 working 栏的差异标记一一对应，对齐 IDEA / SmartGit。 */
.hl-line.side-ours {
  background-color: var(--bg-del);
}

.hl-line.side-theirs {
  background-color: var(--bg-add);
}

/* 当前冲突块：在三栏原有底色之上再加一层强调，便于在长文件里定位（IDEA 的当前冲突高亮） */
.hl-line.current.line-confl-ours,
.hl-line.side-ours.current {
  background-color: color-mix(in srgb, var(--color-del) 22%, transparent);
}

.hl-line.current.line-confl-theirs,
.hl-line.side-theirs.current {
  background-color: color-mix(in srgb, var(--color-add) 22%, transparent);
}

.hl-line.current.line-confl-marker,
.hl-line.current.line-confl-base {
  background-color: var(--bg-toolbar);
}

/* 词级差异高亮：在行级底色之上，进一步框出「这一行里到底哪几个词不同」（IDEA / SmartGit 风格）。
   色相跟随所在侧：ours = 红（--color-del），theirs = 绿（--color-add）。 */
.seg-chg {
  border-radius: 2px;
}

.line-confl-ours .seg-chg,
.hl-line.side-ours .seg-chg {
  background-color: color-mix(in srgb, var(--color-del) 30%, transparent);
}

.line-confl-theirs .seg-chg,
.hl-line.side-theirs .seg-chg {
  background-color: color-mix(in srgb, var(--color-add) 30%, transparent);
}

/* 两列之间的操作列：与编辑页面(编辑差异)一致，按冲突块行号垂直定位箭头按钮 */
.solver-midcol {
  width: 36px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  background-color: var(--bg-toolbar);
  border-right: 1px solid var(--border-color);
  overflow-y: auto;
  scrollbar-width: none;
}

.solver-midcol::-webkit-scrollbar {
  display: none;
  width: 0;
  height: 0;
}

/* 中间列顶部占位：高度 = 相邻文本栏 .panel-label（运行时实测，midcolTopPad）。
   文本栏的编辑器被 panel-label 整体下压了 L，中间列没有 label，故补一个等高的
   非滚动占位（sticky 不随滚动移走），把弧形/箭头按钮的垂直原点对齐到编辑器内容原点，
   修正原「弧形整体偏高 ≈L」的错位。占位用与 panel-label 一致的灰底+下边框，滚动时
   与文本栏一样把上方内容遮在条带之下，保证三栏顶部视觉对齐。 */
.midcol-label-spacer {
  position: sticky;
  top: 0;
  flex-shrink: 0;
  z-index: 2;
  box-sizing: border-box;
  background-color: var(--bg-toolbar);
  border-bottom: 1px solid var(--border-color);
}

.midcol-inner {
  position: relative;
  /* 顶部 8px：与 textarea 内容起点一致（行 0 上沿对齐） */
  padding-top: 8px;
}

.solver-action-group {
  position: absolute;
  left: 0;
  right: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  /* 置于弧形背景色标记（.diff-bands，z-index:0）之上，避免背景盖住箭头按钮 */
  z-index: 1;
}

/* 中间列弧形背景色标记（与编辑页 .diff-bands 同款 S 曲线漏斗）：绝对定位于 midcol-inner
   顶部，随中间列滚动；纯装饰，不拦截事件。坐标与 .solver-action-group 的 top 同基准 */
.diff-bands {
  position: absolute;
  top: 0;
  left: 0;
  width: 100%;
  z-index: 0;
  pointer-events: none;
  overflow: visible;
}

/* 与编辑页 .diff-bands .band 完全一致的描边规格 */
.diff-bands .band {
  stroke-width: 1;
  stroke-linejoin: round;
  stroke-opacity: 0.45;
  fill-opacity: 1;
}

/* 连接带配色 = 「它连接的那一侧的差异色」，与同侧行底色同一套语义
   （ours = 红/删 --color-del，与 .line-confl-ours 的 --bg-del 同源；
     theirs = 绿/增 --color-add，与 .line-confl-theirs 的 --bg-add 同源），
   与编辑页 .band-del / .band-add 也是同一套色。这样中间列的带子读作
   「这一块差异顺着缺口延续过去」，而不是「某个动作按钮的颜色」。
   注意：中间列底色 --bg-toolbar(#f1f5f9) 比 ours/theirs 栏底色 --bg-tertiary(#e2e8f0) 亮，
   同一 alpha 在中间列会显得更淡，故这里 alpha 取比行底色略高的值以保证「桥」可见。 */

/* 左中列（ours ↔ working）：红 = 该块 ours 内容（删/HEAD 侧） */
.diff-bands.band-ours .band {
  fill: color-mix(in srgb, var(--color-del, #dc2626) 22%, transparent);
  stroke: var(--color-del, #dc2626);
}

.diff-bands.band-ours .band.active {
  fill: color-mix(in srgb, var(--color-del, #dc2626) 34%, transparent);
  stroke-opacity: 0.85;
}

/* 右中列（working ↔ theirs）：绿 = 该块 theirs 内容（增/合并侧） */
.diff-bands.band-theirs .band {
  fill: color-mix(in srgb, var(--color-add, #16a34a) 22%, transparent);
  stroke: var(--color-add, #16a34a);
}

.diff-bands.band-theirs .band.active {
  fill: color-mix(in srgb, var(--color-add, #16a34a) 34%, transparent);
  stroke-opacity: 0.85;
}

.merge-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  border: none;
  border-radius: 4px;
  background: transparent;
  cursor: pointer;
  padding: 0;
  transition: background-color 0.12s ease, transform 0.1s ease;
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

/* 接受 ours → 绿色（与编辑页面 accept-left 一致） */
.merge-btn.accept-left {
  color: var(--color-add, #2ea043);
}

.merge-btn.accept-left:hover {
  background-color: rgba(46, 160, 67, 0.2);
}

/* 接受 theirs → 蓝色（与编辑页面 accept-right 一致） */
.merge-btn.accept-right {
  color: var(--accent-primary, #388bfd);
}

.merge-btn.accept-right:hover {
  background-color: rgba(56, 139, 253, 0.2);
}

.merge-btn:active {
  transform: scale(0.9);
}

.inline-widget {
  position: absolute;
  top: 32px;
  left: 50%;
  transform: translateX(-50%);
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 8px;
  background-color: var(--bg-secondary);
  border: 1px solid var(--border-medium);
  border-radius: 6px;
  box-shadow: var(--shadow-dialog);
  z-index: 10;
}

.widget-label {
  font-size: 10px;
  color: var(--text-tertiary);
  font-weight: 600;
  margin-right: 4px;
  white-space: nowrap;
}

.inline-widget :deep(button) {
  height: 22px;
  padding: 0 8px;
  font-size: 10px;
  gap: 3px;
}
</style>
