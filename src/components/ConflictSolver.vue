<script setup lang="ts">
import { ref, computed, watch, nextTick, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import type { ConflictFile, ConflictBlock } from '../types'
import {
  ChevronUp, ChevronDown, ArrowLeftToLine, ArrowRightToLine,
  AlignJustify, Save, X, GitMerge, ArrowLeft, ArrowRight,
  Combine, Rows3,
} from 'lucide-vue-next'
import { Button } from '@/components/ui/button'
import { writeFileContent } from '../api/git'

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

const allLineCount = computed(() => workingContent.value.split('\n').length)
const leftLineCount = computed(() => props.conflictFile.ours_content.length || 1)
const rightLineCount = computed(() => props.conflictFile.theirs_content.length || 1)

const highlights = computed(() => {
  return blocks.value.map((block, idx) => ({
    top: PADDING_TOP + block.start_line * LINE_HEIGHT,
    height: (block.end_line - block.start_line + 1) * LINE_HEIGHT,
    active: idx === currentBlockIndex.value,
  }))
})

const previewContent = computed(() => {
  let content = workingContent.value
  const list = parseConflictBlocks(content)
  for (let i = list.length - 1; i >= 0; i--) {
    const block = list[i]
    const replacement = block.ours.join('') + block.theirs.join('')
    const lines = splitLinesKeepNewline(content)
    content = [
      ...lines.slice(0, block.start_line),
      replacement,
      ...lines.slice(block.end_line + 1),
    ].join('')
  }
  return content
})

const conflictFree = computed(() => !hasBlocks.value)

watch(currentBlock, async () => {
  await nextTick()
  scrollToCurrentBlock()
})

onMounted(() => {
  scrollToCurrentBlock()
})

function scrollToCurrentBlock() {
  const block = currentBlock.value
  const ta = middleTextarea.value
  if (!block || !ta) return
  const targetTop = PADDING_TOP + block.start_line * LINE_HEIGHT
  ta.scrollTop = Math.max(0, targetTop - ta.clientHeight / 3)
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
  const sourceEl = refs[source]
  if (!sourceEl) return
  const scrollTop = sourceEl.scrollTop
  const scrollLeft = sourceEl.scrollLeft

  ;(Object.keys(refs) as Array<keyof typeof refs>).forEach((key) => {
    const el = refs[key]
    const lineEl = lineRefs[key]
    if (el && el !== sourceEl) {
      el.scrollTop = scrollTop
      el.scrollLeft = scrollLeft
    }
    if (lineEl && lineEl !== sourceEl) {
      lineEl.scrollTop = scrollTop
    }
  })
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
            <textarea
              ref="middleTextarea"
              v-model="workingContent"
              class="solver-textarea"
              spellcheck="false"
              @scroll="syncScroll('middle')"
            />
            <div class="highlight-overlays">
              <div
                v-for="(h, idx) in highlights"
                :key="idx"
                class="conflict-highlight"
                :class="{ active: h.active }"
                :style="{ top: h.top + 'px', height: h.height + 'px' }"
              />
            </div>
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

    <!-- 底部合并预览 -->
    <div class="preview-panel">
      <div class="preview-label">
        <Combine :size="11" />
        <span>{{ t('conflictSolver.preview') }}</span>
        <span v-if="conflictFree" class="preview-badge clean">{{ t('conflictSolver.clean') }}</span>
        <span v-else class="preview-badge pending">{{ t('conflictSolver.pendingBlocks', { n: blocks.length }) }}</span>
      </div>
      <textarea
        :value="previewContent"
        readonly
        class="preview-textarea"
        spellcheck="false"
      />
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
  padding: 8px;
  background-color: var(--bg-secondary);
  color: var(--text-primary);
  white-space: pre;
  overflow: auto;
  box-sizing: border-box;
}

.solver-textarea:focus {
  outline: none;
  box-shadow: inset 0 0 0 1px var(--accent-primary);
}

.solver-textarea:read-only {
  background-color: var(--bg-tertiary);
}

.middle-panel .solver-textarea {
  background-color: var(--bg-primary);
}

.highlight-overlays {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  pointer-events: none;
  overflow: hidden;
}

.conflict-highlight {
  position: absolute;
  left: 0;
  right: 0;
  background-color: rgba(244, 71, 71, 0.08);
  border-left: 2px solid var(--color-del);
}

.conflict-highlight.active {
  background-color: rgba(244, 71, 71, 0.16);
  border-left: 2px solid var(--color-del);
  box-shadow: inset 0 0 0 1px rgba(244, 71, 71, 0.25);
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

.preview-panel {
  height: 120px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  border-top: 1px solid var(--border-color);
  background-color: var(--bg-secondary);
}

.preview-label {
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
}

.preview-badge {
  margin-left: auto;
  font-size: 9px;
  padding: 1px 6px;
  border-radius: 8px;
  text-transform: none;
}

.preview-badge.clean {
  background-color: var(--bg-add);
  color: var(--color-add);
}

.preview-badge.pending {
  background-color: var(--bg-del);
  color: var(--color-del);
}

.preview-textarea {
  flex: 1;
  resize: none;
  border: none;
  border-radius: 0;
  font-family: Consolas, Monaco, 'Courier New', monospace;
  font-size: 11px;
  line-height: 16px;
  padding: 6px 10px;
  background-color: var(--bg-tertiary);
  color: var(--text-primary);
  white-space: pre;
  overflow: auto;
}

.preview-textarea:read-only {
  outline: none;
}
</style>
