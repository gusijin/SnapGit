<script setup lang="ts">
import { ref, computed, watch, nextTick, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { Search, X, GitCommitVertical, History } from 'lucide-vue-next'
import type { Commit } from '../types'

interface Props {
  commits: Commit[]
  selectedId?: string
  // 分页加载状态：滚动到底自动加载更早历史
  loadingMore?: boolean
  noMore?: boolean
}

const props = defineProps<Props>()
const emit = defineEmits(['select-commit', 'show-commit-files', 'exit-log', 'load-more'])
const { t } = useI18n()

const filterText = ref('')

const filteredCommits = computed(() => {
  const q = filterText.value.trim().toLowerCase()
  if (!q) return props.commits
  return props.commits.filter(c =>
    c.message.toLowerCase().includes(q) ||
    c.author.toLowerCase().includes(q) ||
    c.id.toLowerCase().includes(q)
  )
})

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
  <div class="log-viewer">
    <div class="panel-header">
      <History :size="13" class="header-icon" />
      <span>{{ t('logView.logTitle') }}</span>
      <span class="count">{{ filteredCommits.length }}</span>
      <div class="search-box">
        <Search :size="13" class="search-icon" />
        <input
          v-model="filterText"
          class="search-input"
          type="text"
          :placeholder="t('logView.searchPlaceholder')"
        />
        <button v-if="filterText" class="search-clear" @click="filterText = ''" :title="t('logView.clearSearch')">
          <X :size="13" />
        </button>
      </div>
    </div>

    <div v-if="filteredCommits.length === 0" class="empty">
      <GitCommitVertical :size="42" class="empty-icon" />
      <p>{{ filterText ? t('logView.noMatch') : t('logView.noCommits') }}</p>
    </div>

    <div v-else class="log-list">
      <!-- @contextmenu.prevent.stop：日志列表禁用右键，不弹出任何菜单（搜索框不在本容器内，仍可正常右键粘贴） -->
      <div class="log-items" ref="logItemsRef" @scroll="onLogScroll" @contextmenu.prevent.stop>
        <div 
          v-for="commit in filteredCommits" 
          :key="commit.id"
          class="log-item"
          :class="{ selected: commit.id === selectedId }"
          @click="handleSelect(commit.id)"
          @dblclick="emit('show-commit-files', commit.id)"
        >
          <!-- 图形列：ID + 分支点 -->
          <div class="col col-id">
            <span class="commit-dot"></span>
            <span class="commit-id">{{ commit.id.slice(0, 7) }}</span>
          </div>

          <!-- 信息列 -->
          <div class="col col-msg">
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
        <div v-if="loadingMore" class="log-more">加载中...</div>
        <div v-else-if="noMore && commits.length > 0" class="log-more end">已加载全部提交</div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.log-viewer {
  display: flex;
  flex-direction: column;
  height: 100%;
  background-color: var(--bg-secondary);
  min-height: 0;
  /* 提交日志面板内容禁止选中文字，兼容 Windows(WebView2) 与 macOS(WKWebView) */
  -webkit-user-select: none;
  -moz-user-select: none;
  user-select: none;
}

.panel-header {
  display: flex;
  align-items: center;
  gap: 6px;
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

.header-icon {
  color: var(--accent-text);
  flex-shrink: 0;
}

.count {
  font-size: 10px;
  padding: 1px 6px;
  background-color: var(--bg-tertiary);
  border-radius: 8px;
  color: var(--text-tertiary);
}

.search-box {
  position: relative;
  margin-left: auto;
  display: flex;
  align-items: center;
}

.search-icon {
  position: absolute;
  left: 7px;
  color: var(--text-tertiary);
  pointer-events: none;
}

.search-input {
  width: 190px;
  max-width: 42vw;
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
  transition: border-color 0.15s, box-shadow 0.15s;
}

.search-input::placeholder {
  color: var(--text-muted);
}

.search-input:focus {
  border-color: color-mix(in srgb, var(--accent-primary) 45%, var(--bg-primary));
  box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent-primary) 18%, transparent);
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
  grid-template-columns: 110px 1fr 150px 150px;
  gap: 12px;
  padding: 8px 12px;
  min-height: 38px;
  line-height: 1.4;
  align-items: center;
  border-bottom: 1px solid var(--border-light);
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

.col-id {
  display: flex;
  align-items: center;
  gap: 6px;
}

.commit-dot {
  width: 10px;
  height: 10px;
  flex-shrink: 0;
  border-radius: 50%;
  background: var(--text-tertiary);
  border: 2px solid var(--bg-secondary);
  box-shadow: 0 0 0 1px var(--border-medium);
}

.commit-id {
  font-family: Consolas, Monaco, monospace;
  font-size: 11px;
  font-weight: 600;
  color: #111111;
  background-color: #e5e7eb;
  padding: 1px 7px;
  border-radius: 10px;
  white-space: nowrap;
}

.commit-message {
  font-size: 13px;
  color: var(--text-primary);
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
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
</style>
