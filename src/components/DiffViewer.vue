<script setup lang="ts">
import { ref, computed, watch, nextTick, onMounted, onUnmounted } from 'vue'
import { useI18n } from 'vue-i18n'
import type { FileDiff } from '../types'
import {
  FileQuestion, CheckCircle2, GitCompare, History, Pencil, FileArchive, FileX,
} from 'lucide-vue-next'

interface Props {
  diff: FileDiff | null
  filePath: string
  loading?: boolean
  viewMode?: 'working-tree' | 'commit'
  autoScroll?: boolean
}

const props = defineProps<Props>()
const { t } = useI18n()

const leftScrollRef = ref<HTMLElement | null>(null)
const rightScrollRef = ref<HTMLElement | null>(null)
const leftInnerRef = ref<HTMLElement | null>(null)
const rightInnerRef = ref<HTMLElement | null>(null)
const midBandsRef = ref<SVGElement | null>(null)

// 程序化回写会引发对端 scroll 回响，用「最近一次程序化写入的目标元素 + 120ms 窗口」吞掉，
// 避免 1:1 滚动同步下的乒乓回写。
let programmaticTarget: HTMLElement | null = null
let programmaticTime = 0

// 强制左右滚动容器 scrollHeight 一致：文件末尾存在「仅一侧有行」的 trailing newline / 空行差异时，
// 较短一侧会在 .side-inner 补 padding，使两侧可滚动高度相同，1:1 scrollTop 同步不会在某侧提前触底。
const BASE_INNER_PADDING = 10

function equalizeScrollHeights() {
  const leftEl = leftScrollRef.value
  const rightEl = rightScrollRef.value
  const leftInner = leftInnerRef.value
  const rightInner = rightInnerRef.value
  if (!leftEl || !rightEl || !leftInner || !rightInner) return

  // 先复位为 CSS 默认 padding，再测量「自然」scrollHeight，避免上一轮补偿残留叠加
  leftInner.style.paddingBottom = ''
  rightInner.style.paddingBottom = ''

  // 读 scrollHeight 会强制同步回流，复位立即生效
  const leftH = leftEl.scrollHeight
  const rightH = rightEl.scrollHeight
  if (leftH === rightH) return

  // 给较短一侧补 padding，使两侧可滚动高度一致（BASE_INNER_PADDING 为 CSS 默认值，diff 为差额）
  const diff = Math.abs(leftH - rightH)
  if (leftH < rightH) {
    leftInner.style.paddingBottom = `${BASE_INNER_PADDING + diff}px`
  } else {
    rightInner.style.paddingBottom = `${BASE_INNER_PADDING + diff}px`
  }
}

function syncScroll(source: 'left' | 'right') {
  const sourceEl = source === 'left' ? leftScrollRef.value : rightScrollRef.value
  const targetEl = source === 'left' ? rightScrollRef.value : leftScrollRef.value
  if (!sourceEl || !targetEl) return

  // 吞掉「我们刚程序化写入」触发回来的 scroll（对端回响），否则会反向改写源侧、产生错位
  if (programmaticTarget === sourceEl && performance.now() - programmaticTime < 120) {
    programmaticTarget = null
    return
  }

  // 查看面板左右两栏渲染同一份 renderLines（行数严格一致、每行固定 18px）。
  // equalizeScrollHeights() 已在 diff 渲染后补齐两侧可滚动高度，使 scrollHeight 一致，
  // 因此直接 1:1 拷贝 scrollTop 即可保证相同代码行永远在同一水平线。
  // ⚠️ 勿改用 computeAnchoredScrollTop 锚定：那是给「两侧行数可能不等」的编辑窗口设计的；
  // 在连续 add/delete（一侧空白 stretch）处锚点钳制会把对端停在「上一锚点 + 半行」，
  // 导致左右相同行错开数行（古哥 2026-09-04 反馈过，已改 1:1 + 高度补齐，勿回退）。
  if (targetEl.scrollTop !== sourceEl.scrollTop) {
    programmaticTarget = targetEl
    programmaticTime = performance.now()
    targetEl.scrollTop = sourceEl.scrollTop
  }
  if (targetEl.scrollLeft !== sourceEl.scrollLeft) targetEl.scrollLeft = sourceEl.scrollLeft

  // 中间连接带图层跟随垂直滚动（用源侧真实 scrollTop，保证连接带不抖动）
  if (midBandsRef.value) {
    midBandsRef.value.style.transform = `translateY(${-sourceEl.scrollTop}px)`
  }
}

// 自动滚动到第一个变更行（异常位置）并高亮闪烁
// 仅在 autoScroll 为 true（用户主动切换文件）时滚动，自动刷新时保持滚动位置
const ROW_HEIGHT = 18 // 行高：与编辑文件对比（DiffEditor）一致
const firstChangeIdx = ref(-1)

// 渲染上限：差异行数超过阈值时只渲染前 N 行并提示截断。
// 超大文件（如压缩后的 js、生成产物）的 diff 可达数万行，一次性渲染
// 左右两份完整 DOM 会让主线程长时间停顿，表现为整窗冻结/白屏后突然恢复。
const MAX_RENDER_LINES = 6000
const truncated = computed(() => !!props.diff && props.diff.lines.length > MAX_RENDER_LINES)
const renderLines = computed(() => {
  if (!props.diff) return []
  return props.diff.lines.length > MAX_RENDER_LINES
    ? props.diff.lines.slice(0, MAX_RENDER_LINES)
    : props.diff.lines
})

watch(
  () => props.diff,
  async (newDiff) => {
    firstChangeIdx.value = -1
    if (!newDiff || newDiff.lines.length === 0) return
    // 自动刷新（autoScroll=false）时不重算 firstChangeIdx，避免高亮闪烁动画
    if (props.autoScroll === false) return
    // 只在可见范围内找第一个变更行（超出渲染上限的部分不参与滚动定位）
    const limit = Math.min(newDiff.lines.length, MAX_RENDER_LINES)
    let idx = -1
    for (let i = 0; i < limit; i++) {
      if (newDiff.lines[i].line_type !== 'context') {
        idx = i
        break
      }
    }
    if (idx === -1) return
    firstChangeIdx.value = idx
    await nextTick()
    equalizeScrollHeights()
    const rightEl = rightScrollRef.value
    const leftEl = leftScrollRef.value
    if (rightEl) {
      const scrollTo = Math.max(0, idx * ROW_HEIGHT - ROW_HEIGHT * 3)
      rightEl.scrollTop = scrollTo
      if (leftEl) {
        leftEl.scrollTop = scrollTo
      }
    }
  }
)

// diff 渲染完成后同步两侧可滚动高度（不受 autoScroll 开关影响）
watch(
  () => props.diff,
  async () => {
    await nextTick()
    equalizeScrollHeights()
  },
  { immediate: true }
)

onMounted(() => {
  window.addEventListener('resize', equalizeScrollHeights)
  equalizeScrollHeights()
})

onUnmounted(() => {
  window.removeEventListener('resize', equalizeScrollHeights)
})

function getLineTypeClass(type: string): string {
  switch (type) {
    case 'add': return 'line-add'
    case 'delete': return 'line-del'
    case 'modified': return 'line-modified'
    default: return 'line-context'
  }
}

// 差异位置标记：可见范围内的所有非 context 行
const diffMarkers = computed(() => {
  return renderLines.value
    .map((line, idx) => ({ idx, type: line.line_type }))
    .filter(m => m.type !== 'context')
})

const totalLines = computed(() => renderLines.value.length)

// ---- Beyond Compare / IDEA 风格差异连接带 ----
// 连续非 context 行合并为一个差异块（hunk），分别取左/右侧「有内容行」的视觉纵向范围，
// 在中间 gutter 画弧形四边形（S 曲线上下沿）。两侧行数不同时形成梯形/漏斗；
// 纯新增/删除时无内容一侧塌缩为块中点。
interface DiffHunk {
  kind: 'del' | 'add' | 'mod'
  path: string
}

const GUTTER_W = 26 // 中间 gutter 宽度（与 .middle-gutter CSS 一致）

const bandsHeight = computed(() => renderLines.value.length * ROW_HEIGHT)

const hunks = computed<DiffHunk[]>(() => {
  const lines = renderLines.value
  const result: DiffHunk[] = []
  const x0 = 0.75              // 左边缘（内缩避免描边裁剪）
  const x1 = GUTTER_W - 0.75   // 右边缘
  const c = GUTTER_W / 2       // 贝塞尔控制点 x：两端水平切线
  let i = 0
  while (i < lines.length) {
    if (lines[i].line_type === 'context') { i++; continue }
    let j = i + 1
    while (j < lines.length && lines[j].line_type !== 'context') j++
    // hunk = [i, j)：add 行仅右侧有内容，delete 行仅左侧有内容
    let lFirst = -1, lLast = -1, rFirst = -1, rLast = -1
    let hasDel = false, hasMod = false
    for (let k = i; k < j; k++) {
      const lt = lines[k].line_type
      if (lt !== 'add') { if (lFirst < 0) lFirst = k; lLast = k }
      if (lt !== 'delete') { if (rFirst < 0) rFirst = k; rLast = k }
      if (lt === 'delete') hasDel = true
      else if (lt === 'modified') hasMod = true
    }
    const mid = (i * ROW_HEIGHT + j * ROW_HEIGHT) / 2 // 无内容侧塌缩点：块垂直中点
    const lTop = lFirst >= 0 ? lFirst * ROW_HEIGHT : mid
    const lBot = lFirst >= 0 ? (lLast + 1) * ROW_HEIGHT : mid
    const rTop = rFirst >= 0 ? rFirst * ROW_HEIGHT : mid
    const rBot = rFirst >= 0 ? (rLast + 1) * ROW_HEIGHT : mid
    const kind: DiffHunk['kind'] = hasMod ? 'mod' : hasDel ? 'del' : 'add'
    result.push({
      kind,
      path: [
        `M ${x0} ${lTop}`,
        `C ${c} ${lTop} ${c} ${rTop} ${x1} ${rTop}`,
        `L ${x1} ${rBot}`,
        `C ${c} ${rBot} ${c} ${lBot} ${x0} ${lBot}`,
        'Z',
      ].join(' '),
    })
    i = j
  }
  return result
})

function markerClass(type: string): string {
  switch (type) {
    case 'add': return 'marker-add'
    case 'delete': return 'marker-del'
    case 'modified': return 'marker-mod'
    default: return ''
  }
}

// 按行号比例计算标记条位置与高度，保证位置精确
function rulerStyle(idx: number): { top: string; height: string } {
  const total = totalLines.value
  if (total <= 0) return { top: '0%', height: '0%' }
  const height = Math.max(2, 100 / total)
  const top = (idx / total) * 100
  return { top: `${top}%`, height: `${height}%` }
}

const fileName = () => {
  // 使用 props 避免警告
  void props.diff
  if (!props.filePath) return ''
  const parts = props.filePath.replace(/\\/g, '/').split('/')
  return parts[parts.length - 1] || props.filePath
}
</script>

<template>
  <!-- @contextmenu.prevent：差异面板内禁用右键，不弹出任何菜单 -->
  <div class="diff-viewer" @contextmenu.prevent>
    <div class="panel-header">
      <GitCompare :size="13" class="header-icon" />
      <span>{{ t('diffViewer.title') }}</span>
      <span v-if="filePath" class="file-title">{{ fileName() }}</span>
    </div>

    <div v-if="loading" class="empty">
      <div class="spinner"></div>
      <p>{{ t('diffViewer.loading') }}</p>
    </div>

    <div v-else-if="!diff" class="empty">
      <FileQuestion :size="48" class="empty-icon" />
      <p>{{ t('diffViewer.selectHint') }}</p>
    </div>

    <!-- 二进制文件（exe/图片/压缩包）：后端不产出文本 diff，此处显式提示，避免误显示成"无变化" -->
    <div v-else-if="diff.is_binary" class="empty">
      <FileArchive :size="48" class="empty-icon" />
      <p>{{ t('diffViewer.binaryFile') }}</p>
      <p class="empty-sub">{{ t('diffViewer.binaryFileHint') }}</p>
    </div>

    <!-- 超大文本文件：行数超阈值，后端跳过了 LCS 计算避免卡死，此处提示而非渲染空 diff -->
    <div v-else-if="diff.is_oversized" class="empty">
      <FileX :size="48" class="empty-icon" />
      <p>{{ t('diffViewer.oversized') }}</p>
      <p class="empty-sub">{{ t('diffViewer.oversizedHint') }}</p>
    </div>

    <div v-else-if="diff.lines.length === 0 && diff.new_content.length === 0" class="empty">
      <CheckCircle2 :size="48" class="empty-icon" />
      <p>{{ t('diffViewer.noChanges') }}</p>
    </div>

    <div v-else class="diff-content">
      <!-- 表格标题 -->
      <div class="diff-table-header">
        <div class="header-side">
          <History :size="12" class="side-icon" />
          <span class="header-title">{{ viewMode === 'commit' ? t('diffViewer.oldParentCommit') : t('diffViewer.oldHead') }}</span>
        </div>
        <div class="header-divider"></div>
        <div class="header-side">
          <Pencil :size="12" class="side-icon" />
          <span class="header-title">{{ viewMode === 'commit' ? t('diffViewer.newThisCommit') : t('diffViewer.newWorkingTree') }}</span>
        </div>
      </div>

      <!-- 左右对比表格 -->
      <div class="diff-container">
        <div class="side-wrapper">
          <div
            class="side left-side"
            ref="leftScrollRef"
            @scroll="syncScroll('left')"
          >
            <div
              class="side-inner"
              ref="leftInnerRef"
            >
              <div
                v-for="(line, idx) in renderLines"
                :key="idx"
                class="table-row"
                :class="[line.line_type !== 'add' ? getLineTypeClass(line.line_type) : '', {
                  'empty': line.line_type === 'add',
                  'first-change': idx === firstChangeIdx
                }]"
              >
                <span class="line-num">{{ line.old_line ?? '' }}</span>
                <span class="line-content">
                  <template v-if="line.line_type === 'modified'">
                    <span
                      v-for="(seg, si) in line.old_segments"
                      :key="si"
                      :class="seg.changed ? 'seg-old-changed' : ''"
                    >{{ seg.text }}</span>
                  </template>
                  <template v-else>{{ line.line_type === 'add' ? '' : line.content }}</template>
                </span>
              </div>
            </div>
          </div>
        </div>

        <!-- 中间 gutter：Beyond Compare / IDEA 风格差异连接带（纯装饰，随滚动 translateY 同步） -->
        <div class="middle-gutter" aria-hidden="true">
          <svg
            v-if="renderLines.length > 0"
            ref="midBandsRef"
            class="diff-bands"
            :viewBox="`0 0 ${GUTTER_W} ${bandsHeight}`"
            :style="{ height: bandsHeight + 'px' }"
            preserveAspectRatio="none"
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
        </div>

        <div class="side-wrapper">
          <div
            class="side right-side"
            ref="rightScrollRef"
            @scroll="syncScroll('right')"
          >
            <div
              class="side-inner"
              ref="rightInnerRef"
            >
              <div
                v-for="(line, idx) in renderLines"
                :key="idx"
                class="table-row"
                :class="[line.line_type !== 'delete' ? getLineTypeClass(line.line_type) : '', {
                  'empty': line.line_type === 'delete',
                  'first-change': idx === firstChangeIdx
                }]"
              >
                <span class="line-num">{{ line.new_line ?? '' }}</span>
                <span class="line-content">
                  <template v-if="line.line_type === 'modified'">
                    <span
                      v-for="(seg, si) in line.new_segments"
                      :key="si"
                      :class="seg.changed ? 'seg-new-changed' : ''"
                    >{{ seg.text }}</span>
                  </template>
                  <template v-else>{{ line.line_type === 'delete' ? '' : line.content }}</template>
                </span>
              </div>
            </div>
          </div>
          <!-- 差异位置标记条（固定，不随滚动移动） -->
          <div class="overview-ruler">
            <div
              v-for="m in diffMarkers"
              :key="m.idx"
              class="ruler-mark"
              :class="markerClass(m.type)"
              :style="rulerStyle(m.idx)"
            ></div>
          </div>
        </div>
      </div>

      <!-- 超大文件截断提示 -->
      <div v-if="truncated" class="diff-truncated">
        {{ t('diffViewer.truncated', { n: MAX_RENDER_LINES }) }}
      </div>
    </div>
  </div>
</template>

<style scoped>
.diff-viewer {
  display: flex;
  flex-direction: column;
  height: 100%;
  background-color: var(--bg-secondary);
  min-height: 0;
}

.panel-header {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 6px 10px;
  background-color: var(--bg-toolbar);
  border-bottom: 1px solid var(--border-color);
  font-size: 11px;
  color: var(--text-secondary);
  font-weight: 600;
  text-transform: uppercase;
  flex-shrink: 0;
  -webkit-user-select: none;
  -webkit-user-drag: none;
  user-select: none;
}

.file-title {
  font-family: Consolas, Monaco, monospace;
  color: var(--accent-text);
  font-weight: 500;
  text-transform: none;
}

.header-icon {
  color: var(--accent-text);
  flex-shrink: 0;
}

.side-icon {
  color: var(--text-muted);
  flex-shrink: 0;
}

.empty {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  color: var(--text-muted);
  padding: 40px;
  min-height: 0;
}

.empty-icon {
  color: var(--text-muted);
  opacity: 0.5;
}

.spinner {
  width: 30px;
  height: 30px;
  border: 3px solid var(--border-medium);
  border-top-color: var(--accent-primary);
  border-radius: 50%;
  animation: diff-spin 0.8s linear infinite;
}

@keyframes diff-spin {
  to { transform: rotate(360deg); }
}

.empty p {
  margin: 0;
  font-size: 13px;
}

/* 二进制提示的副标题（选择器需压过 `.empty p` 的 13px） */
.empty p.empty-sub {
  margin: 0;
  font-size: 11px;
  opacity: 0.75;
  max-width: 340px;
  text-align: center;
  line-height: 1.6;
}

.diff-content {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-height: 0;
  overflow: hidden;
}

.diff-table-header {
  display: flex;
  background-color: var(--bg-toolbar);
  border-bottom: 1px solid var(--border-color);
  flex-shrink: 0;
}

.header-side {
  flex: 1;
  display: flex;
  align-items: center;
  gap: 5px;
  min-width: 0;
  padding: 5px 10px;
  font-size: 10px;
  color: var(--text-tertiary);
  font-weight: 600;
  text-transform: uppercase;
  white-space: nowrap;
  overflow: hidden;
}

.header-divider {
  width: 1px;
  background-color: var(--border-color);
  flex-shrink: 0;
}

.diff-container {
  flex: 1;
  display: flex;
  min-height: 0;
}

.side-wrapper {
  position: relative;
  flex: 1;
  min-width: 0;
  display: flex;
}

/* 中间 gutter：Beyond Compare / IDEA 风格连接带容器，隐藏溢出、不占滚动条 */
.middle-gutter {
  width: 26px; /* 与脚本 GUTTER_W 一致 */
  flex-shrink: 0;
  position: relative;
  overflow: hidden;
}

.diff-bands {
  position: absolute;
  top: 0;
  left: 0;
  width: 100%;
  pointer-events: none;
  will-change: transform;
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

.side {
  flex: 1;
  overflow-y: auto;
  /* 横向滚动条常驻（IDEA 式）：避免一侧内容宽出现横滚条、另一侧没有
     导致两栏视口高度差 10px，滚到底部附近 scrollTop 被钳制而行错位 */
  overflow-x: scroll;
  min-width: 0;
}

.side::-webkit-scrollbar {
  width: 10px;
  height: 10px;
}

.side::-webkit-scrollbar-track {
  background: var(--bg-tertiary);
}

.side::-webkit-scrollbar-thumb {
  background: var(--border-medium);
  border-radius: 5px;
}

.side::-webkit-scrollbar-thumb:hover {
  background: var(--text-muted);
}

.side-inner {
  display: flex;
  flex-direction: column;
  min-height: 100%;
  /* 补偿 .side 的 overflow-x:scroll 常驻横滚条占用的底部 10px，
     否则滚到最底时末行被横滚条盖住一截（与编辑窗口 .editor-textarea 同根因）。
     div 滚动容器的 padding 不会增加 scrollHeight，故补偿必须加在内容层。 */
  padding-bottom: 10px;
}

.left-side {
  border-right: 1px solid var(--border-color);
}

/* 旧版本隐藏垂直滚动条，仅保留水平滚动条；垂直滚动由新版本控制 */
.left-side::-webkit-scrollbar {
  width: 0;
  height: 10px;
}

.table-row {
  display: flex;
  align-items: flex-start;
  gap: 4px;
  height: 18px;
  line-height: 18px;
  font-size: 12px;
  font-family: Consolas, Monaco, 'Courier New', monospace;
  white-space: pre;
  padding: 0 4px 0 0; /* 左侧不留白，行号 gutter 顶到面板边缘（与编辑文件对比一致） */
  box-sizing: border-box;
  border-bottom: 1px solid transparent;
}

.table-row.empty {
  color: transparent;
  min-height: 18px; /* 空占位行（add/delete 对侧）不得被压缩，保证两侧行高一致 */
}

/* 行号列：中性 gutter（与编辑文件对比一致，不随差异行染色） */
.line-num {
  width: 40px;
  flex-shrink: 0;
  text-align: right;
  color: var(--text-muted);
  font-size: 10px;
  padding-right: 6px;
  -webkit-user-select: none;
  -webkit-user-drag: none;
  user-select: none;
  background-color: var(--bg-toolbar);
  border-right: 1px solid var(--border-color);
}

.line-content {
  flex: 1;
  padding-left: 6px;
  min-width: 0;
  overflow-x: visible;
  word-break: break-all;
}

.line-context {
  background-color: var(--bg-secondary);
  color: var(--text-primary);
}

/* 行级只铺底色；文字着色限定在内容区，行号列保持中性 */
.line-add {
  background-color: var(--bg-add);
}

.line-add .line-content {
  color: var(--color-add);
}

.line-del {
  background-color: var(--bg-del);
}

.line-del .line-content {
  color: var(--color-del);
}

.line-modified {
  background-color: var(--bg-tertiary);
  color: var(--text-primary);
}

.seg-old-changed {
  color: var(--color-del);
}

.seg-new-changed {
  color: var(--color-add);
}

.line-context:hover {
  background-color: var(--bg-hover);
}

/* 第一个变更行高亮闪烁 */
.first-change {
  animation: flash-highlight 1.2s ease-out;
}

@keyframes flash-highlight {
  0% { box-shadow: inset 0 0 0 20px rgba(255, 210, 0, 0.35); }
  60% { box-shadow: inset 0 0 0 20px rgba(255, 210, 0, 0.15); }
  100% { box-shadow: none; }
}

/* 差异位置标记条（紧贴垂直滚动条左侧） */
.overview-ruler {
  position: absolute;
  top: 0;
  right: 0;
  bottom: 0;
  width: 5px;
  pointer-events: none;
  overflow: hidden;
}

.ruler-mark {
  position: absolute;
  left: 0;
  width: 100%;
}

.ruler-mark.marker-add {
  background-color: var(--color-add);
}

.ruler-mark.marker-del {
  background-color: var(--color-del);
}

.ruler-mark.marker-mod {
  background-color: var(--accent-primary);
}

/* 超大文件截断提示 */
.diff-truncated {
  flex-shrink: 0;
  padding: 5px 10px;
  background-color: var(--bg-toolbar);
  border-top: 1px solid var(--border-color);
  color: var(--text-tertiary);
  font-size: 11px;
  text-align: center;
}
</style>