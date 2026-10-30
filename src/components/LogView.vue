<script setup lang="ts">
import { ref, computed, watch, nextTick, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { Search, X, GitCommitVertical, GitBranch, Tag, History, ArrowLeft, ChevronDown, ChevronRight } from 'lucide-vue-next'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip'
import { computeCommitGraph, GRAPH_ROW_HEIGHT, GRAPH_LANE_WIDTH, type GraphCurve } from '../utils/commitGraph'
import type { Commit } from '../types'

interface Props {
  commits: Commit[]
  selectedId?: string
  // 分页加载状态：滚动到底自动加载更早历史
  loadingMore?: boolean
  noMore?: boolean
  // 是否在面板头部显示「返回工作区」按钮（仅历史视图需要，工作视图底部的日志小面板不需要）
  showExitButton?: boolean
  // 折叠时是否收成竖向窄条（侧边栏）：仅历史视图左侧日志面板用，
  // 让出宽度给右侧提交详情；工作区视图底部面板为纵向折叠，不需要。
  collapseToSide?: boolean
}

const props = defineProps<Props>()
const emit = defineEmits(['select-commit', 'open-history', 'exit-log', 'load-more'])
const { t } = useI18n()

// 折叠状态：父组件通过 v-model:collapsed 接管（用于同步调整外层 flex 分配），
// 组件内点击标题栏切换。默认展开——提交日志是主内容面板，不应默认藏起来。
const collapsed = defineModel<boolean>('collapsed', { default: false })

// 点击标题栏：切换折叠/展开（与子模块面板一致）
function toggleCollapsed() {
  collapsed.value = !collapsed.value
}

// 历史视图头部「返回工作区」按钮触发，交父组件收尾（Repository.exitLogView）
function exitLog() {
  emit('exit-log')
}

const filterText = ref('')
// 搜索框展开状态：false=收起（标题栏只显示搜索图标按钮），true=展开（显示输入框）
const searchOpen = ref(false)
const searchInputRef = ref<HTMLInputElement | null>(null)

// 点击标题栏搜索图标：展开输入框并自动聚焦
function openSearch() {
  searchOpen.value = true
  nextTick(() => searchInputRef.value?.focus())
}

// 输入框内的 X 按钮：有文本→清除并重新聚焦；无文本→收起搜索框回到图标态
function onSearchClear() {
  if (filterText.value) {
    filterText.value = ''
    nextTick(() => searchInputRef.value?.focus())
  } else {
    searchOpen.value = false
  }
}

const filteredCommits = computed(() => {
  const q = filterText.value.trim().toLowerCase()
  if (!q) return props.commits
  return props.commits.filter(c =>
    c.message.toLowerCase().includes(q) ||
    c.author.toLowerCase().includes(q) ||
    c.id.toLowerCase().includes(q)
  )
})

// 提交流水线：按当前可见列表推导泳道布局（分页追加后整体重算，结果稳定）
const graph = computed(() => computeCommitGraph(filteredCommits.value))
const graphWidth = computed(() => graph.value.lanes * GRAPH_LANE_WIDTH)

function laneX(lane: number): number {
  return lane * GRAPH_LANE_WIDTH + GRAPH_LANE_WIDTH / 2
}

// 合并线：从节点平滑弯到目标泳道下方
function curvePath(curve: GraphCurve): string {
  const x1 = laneX(curve.fromLane)
  const y1 = GRAPH_ROW_HEIGHT / 2
  const x2 = laneX(curve.toLane)
  const y2 = GRAPH_ROW_HEIGHT
  const bend = GRAPH_ROW_HEIGHT * 0.42
  return `M${x1},${y1} C${x1},${y1 + bend} ${x2},${y2 - bend} ${x2},${y2}`
}

function formatDate(dateStr: string): string {
  const timestamp = parseInt(dateStr) * 1000
  if (isNaN(timestamp)) return dateStr
  const date = new Date(timestamp)
  const now = new Date()
  const diffDays = Math.floor((now.getTime() - date.getTime()) / (1000 * 60 * 60 * 24))

  if (diffDays === 0) {
    const diffHours = Math.floor((now.getTime() - date.getTime()) / (1000 * 60 * 60))
    if (diffHours === 0) {
      const diffMins = Math.floor((now.getTime() - date.getTime()) / (1000 * 60))
      if (diffMins < 1) return t('logView.justNow')
      return t('logView.minAgo', { n: diffMins })
    }
    return t('logView.hourAgo', { n: diffHours })
  } else if (diffDays === 1) {
    return t('logView.yesterday')
  } else if (diffDays < 7) {
    return t('logView.dayAgo', { n: diffDays })
  } else {
    return date.toLocaleString(i18nLanguageCode(), {
      year: 'numeric',
      month: 'numeric',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit'
    })
  }
}

function i18nLanguageCode(): string {
  const code = useI18n().locale.value
  // zh-CN → zh-CN；其余语言使用其本身
  return code
}

function getAvatarColor(name: string): string {
  let hash = 0
  for (let i = 0; i < name.length; i++) {
    hash = name.charCodeAt(i) + ((hash << 5) - hash)
  }
  const colors = [
    '#FF6B6B', '#4ECDC4', '#45B7D1', '#96CEB4',
    '#FFEAA7', '#DDA0DD', '#98D8C8', '#F7DC6F',
    '#BB8FCE', '#85C1E9', '#F8B500', '#00BCD4'
  ]
  return colors[Math.abs(hash) % colors.length]
}

function getInitial(name: string): string {
  if (!name) return '?'
  const trimmed = name.trim()
  if (!trimmed) return '?'
  return trimmed.charAt(0).toUpperCase()
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
}

// 在文本中高亮匹配查询的关键字（先转义，避免 HTML 注入）
function highlight(text: string, query: string): string {
  const q = query.trim()
  const escaped = escapeHtml(text)
  if (!q) return escaped
  const safeQ = q.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
  try {
    return escaped.replace(new RegExp(`(${safeQ})`, 'gi'), '<mark class="hl">$1</mark>')
  } catch {
    return escaped
  }
}

function handleSelect(id: string) {
  emit('select-commit', id)
}

// ===== 滚动分页：滚动到底自动加载更早的提交 =====
const logItemsRef = ref<HTMLElement | null>(null)

function onLogScroll() {
  const el = logItemsRef.value
  if (!el) return
  if (el.scrollTop + el.clientHeight >= el.scrollHeight - 60) {
    emit('load-more')
  }
}

// 内容不足一屏时自动补拉（避免永远触发不了滚动）
function checkFill() {
  const el = logItemsRef.value
  if (!el) return
  if (el.scrollHeight <= el.clientHeight + 60 && !props.loadingMore && !props.noMore) {
    emit('load-more')
  }
}

watch(() => props.commits.length, () => {
  nextTick(checkFill)
})

onMounted(() => {
  nextTick(checkFill)
})
</script>

<template>
  <div class="log-viewer" :class="{ collapsed, 'side-collapsed': collapsed && collapseToSide }">
    <!-- 标题栏：点击整栏切换折叠/展开（与子模块面板一致）；箭头指示当前状态 -->
    <div class="panel-header" :class="{ 'side-collapsed': collapsed && collapseToSide }" @click="toggleCollapsed">
      <!-- 折叠/展开指示箭头：展开 ChevronDown / 折叠 ChevronRight，与子模块面板一致 -->
      <button class="header-collapse" :title="collapsed ? t('logView.expand') : t('logView.collapse')">
        <ChevronDown v-if="!collapsed" :size="13" />
        <ChevronRight v-else :size="13" />
      </button>
      <TooltipProvider v-if="showExitButton" :delay-duration="260">
        <Tooltip>
          <TooltipTrigger as-child>
            <button class="header-exit-btn" :aria-label="t('logView.backToWorkspace')" @click.stop="exitLog">
              <ArrowLeft :size="13" />
            </button>
          </TooltipTrigger>
          <TooltipContent side="bottom">{{ t('logView.backToWorkspace') }}</TooltipContent>
        </Tooltip>
      </TooltipProvider>
      <History :size="13" class="header-icon" />
      <span class="log-title-text">{{ t('logView.logTitle') }}</span>
      <span class="count">{{ filteredCommits.length }}</span>
      <!-- 搜索：图标按钮与输入框同处 .search-area，常驻 DOM，靠容器宽度 + 透明度过渡平滑切换。
           @click.stop 防止点击搜索区域（含展开后的输入框）误触发整栏折叠 -->
      <div class="search-area" :class="{ 'is-open': searchOpen }" @click.stop>
        <TooltipProvider :delay-duration="260">
          <Tooltip>
            <TooltipTrigger as-child>
              <button class="header-search-btn" :aria-label="t('logView.search')" @click="openSearch">
                <Search :size="13" />
              </button>
            </TooltipTrigger>
            <TooltipContent side="bottom">{{ t('logView.search') }}</TooltipContent>
          </Tooltip>
        </TooltipProvider>
        <div class="search-box">
          <Search :size="13" class="search-icon" />
          <input
            ref="searchInputRef"
            v-model="filterText"
            class="search-input"
            type="text"
            :placeholder="t('logView.searchPlaceholder')"
            @keydown.esc="onSearchClear"
          />
          <TooltipProvider :delay-duration="260">
            <Tooltip>
              <TooltipTrigger as-child>
                <button class="search-clear" @click="onSearchClear" :aria-label="t('logView.clearSearch')">
                  <X :size="13" />
                </button>
              </TooltipTrigger>
              <TooltipContent side="bottom">{{ filterText ? t('logView.clearSearch') : t('logView.collapseSearch') }}</TooltipContent>
            </Tooltip>
          </TooltipProvider>
        </div>
      </div>
    </div>

    <div v-show="!collapsed" class="log-body">
    <div v-if="filteredCommits.length === 0" class="empty">
      <GitCommitVertical :size="42" class="empty-icon" />
      <p>{{ filterText ? t('logView.noMatch') : t('logView.noCommits') }}</p>
    </div>

    <div v-else class="log-list">
      <!-- @contextmenu.prevent.stop：日志列表禁用右键，不弹出任何菜单（搜索框不在本容器内，仍可正常右键粘贴） -->
      <div class="log-items" ref="logItemsRef" @scroll="onLogScroll" @contextmenu.prevent.stop>
        <div
          v-for="(commit, index) in filteredCommits"
          :key="commit.id"
          class="log-item"
          :class="{ selected: commit.id === selectedId }"
          @click="handleSelect(commit.id)"
          @dblclick="emit('open-history', commit.id)"
        >
          <!-- 图谱列：SourceTree 风格的泳道/节点/合并连线 -->
          <div class="col-graph" :style="{ width: graphWidth + 'px' }">
            <svg
              :width="graphWidth"
              :height="GRAPH_ROW_HEIGHT"
              class="graph-svg"
              aria-hidden="true"
            >
              <line
                v-for="(line, i) in graph.rows[index].lines"
                :key="'l' + i"
                :x1="laneX(line.lane)"
                :x2="laneX(line.lane)"
                :y1="line.y1"
                :y2="line.y2"
                :stroke="line.color"
                stroke-width="1.6"
                stroke-linecap="round"
              />
              <path
                v-for="(curve, i) in graph.rows[index].curves"
                :key="'c' + i"
                :d="curvePath(curve)"
                :stroke="curve.color"
                stroke-width="1.6"
                fill="none"
                stroke-linecap="round"
              />
              <circle
                :cx="laneX(graph.rows[index].lane)"
                :cy="GRAPH_ROW_HEIGHT / 2"
                r="4"
                :fill="graph.rows[index].color"
                class="graph-node"
              />
            </svg>
          </div>

          <!-- 信息列：分支/标签标记 + 提交信息 -->
          <div class="col col-msg">
            <span
              v-for="ref in commit.refs ?? []"
              :key="ref.type + ':' + ref.name"
              class="ref-chip"
              :class="'ref-' + ref.type"
              :title="ref.name"
            >
              <Tag v-if="ref.type === 'tag'" :size="10" />
              <GitBranch v-else :size="10" />
              <span class="ref-name">{{ ref.name }}</span>
            </span>
            <span class="commit-message" v-html="highlight(commit.message.split('\n')[0], filterText)"></span>
          </div>

          <!-- 作者列 -->
          <div class="col col-author">
            <span 
              class="avatar" 
              :style="{ backgroundColor: getAvatarColor(commit.author) }"
            >{{ getInitial(commit.author) }}</span>
            <span class="author-name" v-html="highlight(commit.author, filterText)"></span>
          </div>

          <!-- 日期列 -->
          <div class="col col-date">
            <span class="commit-date">{{ formatDate(commit.date) }}</span>
          </div>
        </div>
        <!-- 分页加载提示 -->
        <div v-if="loadingMore" class="log-more">{{ t('logView.loadingMore') }}</div>
        <div v-else-if="noMore && commits.length > 0" class="log-more end">{{ t('logView.allLoaded') }}</div>
      </div>
    </div>
    </div>
  </div>
</template>

<style scoped>
.log-viewer {
  display: flex;
  flex-direction: column;
  /* 父级 .log-pane 是 flex row（Repository.vue），作为 flex item 必须显式定宽：
     container-type 的 inline-size containment 会让内容尺寸按 0 计算，缺这行面板会塌成竖条 */
  width: 100%;
  height: 100%;
  background-color: var(--bg-secondary);
  min-height: 0;
  /* 容器查询：面板内宽变窄时据此精简列，优先保留「提交信息 + 作者」 */
  container-type: inline-size;
  /* 提交日志面板内容禁止选中文字，兼容 Windows(WebView2) 与 macOS(WKWebView) */
  -webkit-user-select: none;
  -moz-user-select: none;
  user-select: none;
}

.panel-header {
  display: flex;
  align-items: center;
  gap: 6px;
  /* 高度与其它面板标题栏（提交文件 FileList .panel-header / 仓库 RepositoryList .list-header）严格一致 = 30px。
     这些标题栏是 padding:6px 10px + 内容自适应，内容最高的是计数徽章：
     徽章 font-size:10px × 继承行高 1.5 = 15px + 上下 padding 2px = 17px
     → 6 + 17 + 6 = 29px，再加下方 1px 分隔线 = 30px。
     本面板内容最高的是 24px 搜索框，若沿用 padding:6px 会撑到 37px、比别的标题栏高 7px。
     故改为定高 30px（border-box 含 padding 与 1px 分隔线）→ 内容区 29px，24px 搜索框上下各余 2.5px。
     ⚠️ 若全局行高或徽章样式变化，此处 30px 需同步重算。 */
  height: 30px;
  box-sizing: border-box;
  padding: 0 10px;
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

.header-icon {
  color: var(--accent-text);
  flex-shrink: 0;
}

/* 折叠/展开指示箭头按钮（与 .header-exit-btn 视觉统一，必配 title 提示） */
.header-collapse {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  padding: 0;
  border: none;
  border-radius: 5px;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  transition: background-color 0.15s, color 0.15s;
  flex-shrink: 0;
}

.header-collapse:hover {
  background-color: var(--bg-hover);
  color: var(--text-bright);
}

/* 历史视图头部「返回工作区」按钮（icon-only，必配 Tooltip） */
.header-exit-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  padding: 0;
  margin-right: 2px;
  border: none;
  border-radius: 5px;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  transition: background-color 0.15s, color 0.15s;
  flex-shrink: 0;
}

.header-exit-btn:hover {
  background-color: var(--bg-hover);
  color: var(--text-bright);
}

/* 折叠态内容区：撑满标题栏以外的剩余高度，内部空态/列表各自 flex:1 填充 */
.log-body {
  flex: 1 1 auto;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

/* 历史视图折叠成竖向窄条（侧边栏）：只留箭头 + 竖排标题，让出宽度给右侧详情。
   仅 .log-viewer.side-collapsed（即 collapsed && collapseToSide）时生效。 */
.log-viewer.side-collapsed .panel-header {
  flex-direction: column;
  height: 100%;
  padding: 8px 0;
  gap: 12px;
  justify-content: flex-start;
}
.log-viewer.side-collapsed .header-icon,
.log-viewer.side-collapsed .count,
.log-viewer.side-collapsed .search-area,
.log-viewer.side-collapsed .header-exit-btn {
  display: none;
}
.log-viewer.side-collapsed .log-title-text {
  writing-mode: vertical-rl;
  text-orientation: mixed;
  letter-spacing: 2px;
}

/* 搜索收起态：icon-only 按钮，视觉与交互统一于 .header-exit-btn；
   绝对定位于 .search-area 右侧，靠 opacity + scale 与输入框做平滑切换 */
.header-search-btn {
  position: absolute;
  top: 50%;
  right: 0;
  transform: translateY(-50%);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  padding: 0;
  border: none;
  border-radius: 5px;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  opacity: 1;
  z-index: 2;
  transition: background-color 0.15s, color 0.15s, opacity 0.22s cubic-bezier(0.4, 0, 0.2, 1);
  flex-shrink: 0;
}

.header-search-btn:hover {
  background-color: var(--bg-hover);
  color: var(--text-bright);
}

/* 搜索区域容器：宽度随展开/收起过渡（20px 图标态 ↔ 150px 输入框态），
   实现图标「长」成输入框的平滑效果；右对齐挤占标题栏右侧空间 */
.search-area {
  position: relative;
  margin-left: auto;
  height: 24px;
  width: 20px;
  flex-shrink: 0;
  transition: width 0.22s cubic-bezier(0.4, 0, 0.2, 1);
}

.search-area.is-open {
  width: 150px;
}

.search-area.is-open .header-search-btn {
  opacity: 0;
  pointer-events: none;
}

.count {
  font-size: 10px;
  padding: 1px 6px;
  background-color: var(--bg-tertiary);
  border-radius: 8px;
  color: var(--text-tertiary);
}

.search-box {
  position: absolute;
  top: 0;
  right: 0;
  bottom: 0;
  left: 0;
  display: flex;
  align-items: center;
  overflow: hidden;
  opacity: 0;
  pointer-events: none;
  z-index: 1;
  transition: opacity 0.22s cubic-bezier(0.4, 0, 0.2, 1);
}

.search-area.is-open .search-box {
  opacity: 1;
  pointer-events: auto;
}

.search-icon {
  position: absolute;
  left: 7px;
  color: var(--text-tertiary);
  pointer-events: none;
}

.search-input {
  width: 100%;
  min-width: 0;
  height: 24px;
  padding: 0 26px 0 26px;
  font-size: 12px;
  color: var(--text-primary);
  background-color: var(--bg-primary);
  border: 1px solid var(--border-light);
  border-radius: 6px;
  outline: none;
  text-transform: none;
  font-weight: 400;
  transition: border-color 0.15s;
}

.search-input::placeholder {
  color: var(--text-muted);
}

.search-input:focus {
  border-color: color-mix(in srgb, var(--accent-primary) 45%, var(--bg-primary));
}

.search-clear {
  position: absolute;
  right: 5px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  padding: 0;
  border: none;
  border-radius: 50%;
  background: transparent;
  color: var(--text-tertiary);
  cursor: pointer;
  transition: background-color 0.15s, color 0.15s;
}

.search-clear:hover {
  background-color: var(--bg-hover);
  color: var(--text-bright);
}

/* 搜索命中关键字高亮（v-html 注入，需用 :deep 穿透 scoped） */
.log-items :deep(.hl) {
  background-color: var(--accent-primary);
  color: #fff;
  border-radius: 2px;
  padding: 0 1px;
}

.empty {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  color: var(--text-muted);
  padding: 20px;
  min-height: 0;
}

/* 空状态图标：lucide 线性图标，颜色走主题变量（不再依赖 emoji 的 font-size） */
.empty-icon {
  color: var(--text-muted);
  opacity: 0.4;
}

.empty p {
  margin: 0;
  font-size: 13px;
}

.log-list {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-height: 0;
  overflow: hidden;
}

.log-items {
  flex: 1;
  overflow: auto;
  min-height: 0;
}

.log-more {
  padding: 10px;
  text-align: center;
  font-size: 11px;
  color: var(--text-tertiary);
  -webkit-user-select: none;
  user-select: none;
}

.log-more.end {
  color: var(--text-muted);
}

.log-item {
  display: grid;
  /* 列序：图谱 / 提交信息 / 作者 / 日期。作者列收窄至 110px（原 150px），
     avatar 20px + gap 8px + 名字约 82px，足够显示常见用户名；过长的截断省略。
     高度固定 38px，与 commitGraph.ts 的 GRAPH_ROW_HEIGHT 保持一致（SVG 按此绘制）。
     分隔线用 inset box-shadow 而非 border，保证每行可用高度也是 38px。 */
  grid-template-columns: auto minmax(0, 1fr) 110px 150px;
  gap: 10px;
  padding: 0 12px;
  height: 38px;
  box-sizing: border-box;
  line-height: 1.4;
  align-items: center;
  box-shadow: inset 0 -1px 0 var(--border-light);
  cursor: pointer;
  transition: background-color 0.15s;
  font-size: 13px;
}

.log-item:hover {
  background-color: var(--bg-hover);
}

.log-item.selected,
.log-item.selected:hover {
  background-color: var(--bg-selected);
}

.log-item:active {
  background-color: var(--bg-active);
  color: var(--text-bright);
}

.col {
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 8px;
}

/* 图谱列：svg 定宽（泳道数 × 14px），垂直居中铺满行高 */
.col-graph {
  display: flex;
  align-items: center;
  height: 38px;
  flex-shrink: 0;
  overflow: hidden;
}

.graph-svg {
  display: block;
  overflow: visible;
}

/* 节点描边用面板底色「挖」一圈，连线穿点时保持清晰 */
.graph-node {
  stroke: var(--bg-secondary);
  stroke-width: 2;
}

.col-msg {
  gap: 6px;
  overflow: hidden;
}

.commit-message {
  flex: 1 1 auto;
  min-width: 0;
  font-size: 13px;
  color: var(--text-primary);
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* 分支 / 远程分支 / 标签标记（SourceTree 风格的内联胶囊） */
.ref-chip {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  height: 18px;
  max-width: 180px;
  min-width: 0;
  padding: 0 6px;
  /* 与 SourceTree 一致：分支/标签保持完整，提交信息让位并省略 */
  flex: 0 0 auto;
  border-radius: 4px;
  border: 1px solid transparent;
  font-size: 11px;
  font-weight: 600;
  line-height: 1;
  white-space: nowrap;
  overflow: hidden;
}

.ref-name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.ref-branch {
  color: var(--accent-text);
  background-color: color-mix(in srgb, var(--accent-primary) 14%, transparent);
  border-color: color-mix(in srgb, var(--accent-primary) 38%, transparent);
}

.ref-remote {
  color: var(--text-secondary);
  background-color: var(--bg-tertiary);
  border-color: var(--border-light);
}

.ref-tag {
  color: var(--color-mod);
  background-color: var(--bg-mod);
  border-color: color-mix(in srgb, var(--color-mod) 38%, transparent);
}

.avatar {
  width: 20px;
  height: 20px;
  flex-shrink: 0;
  border-radius: 50%;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  color: #fff;
  font-size: 11px;
  font-weight: 600;
}

.author-name {
  font-size: 12px;
  color: var(--text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.commit-date {
  font-size: 12px;
  color: var(--text-tertiary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  text-align: right;
  width: 100%;
}

.col-date {
  justify-content: flex-end;
}

/* 面板较窄时（如历史视图左侧日志面板在窗口缩小时）：隐藏日期列，
   保留「图谱 + 提交信息 + 作者」，图谱是提交历史的骨架，窄屏也优先展示 */
@container (max-width: 560px) {
  .log-item {
    grid-template-columns: auto minmax(0, 1fr) 100px;
  }
  .col-date {
    display: none;
  }
  /* 窄面板下展开态输入框收敛，避免挤压标题文字 */
  .search-area.is-open {
    width: 110px;
  }
}

/* 极窄面板：远程分支标记让位给提交信息，本地分支与标签仍然保留 */
@container (max-width: 460px) {
  .ref-remote {
    display: none;
  }
}
</style>
