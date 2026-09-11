<script setup lang="ts">
import { ref, reactive, computed, watch, onBeforeUnmount } from 'vue'
import { useI18n } from 'vue-i18n'
import type { SubmoduleInfo } from '../types'
import {
  listSubmodules, updateSubmodules, updateSubmodule, syncSubmodules, removeSubmodule,
} from '../api/git'
import AddSubmoduleDialog from './AddSubmoduleDialog.vue'
import {
  Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import {
  Tooltip, TooltipContent, TooltipProvider, TooltipTrigger,
} from '@/components/ui/tooltip'
import {
  Plus, RefreshCw, Repeat, MoreVertical, FolderOpen, Download, ArrowDownToLine, Trash2,
  ChevronDown, ChevronRight, Package, AlertTriangle,
} from 'lucide-vue-next'

const props = defineProps<{
  repoPath: string
}>()
const emit = defineEmits<{
  (e: 'changed'): void
  (e: 'open-submodule', absPath: string): void
  (e: 'added'): void
}>()

const { t } = useI18n()

const collapsed = ref(false)
const loading = ref(false)
const busy = ref(false)
const error = ref('')
const submodules = ref<SubmoduleInfo[]>([])

// ===== 加载列表 =====
async function loadList() {
  if (!props.repoPath) {
    submodules.value = []
    return
  }
  loading.value = true
  error.value = ''
  try {
    submodules.value = await listSubmodules(props.repoPath)
  } catch (e: any) {
    error.value = typeof e === 'string' ? e : e?.toString?.() || String(e)
    submodules.value = []
  } finally {
    loading.value = false
  }
}

watch(() => props.repoPath, () => { loadList() }, { immediate: true })

// 暴露给父组件在切库 / 文件操作后主动刷新
defineExpose({ refresh: loadList })

// ===== 路径工具 =====
function absPath(p: string): string {
  return props.repoPath.replace(/[\\/]+$/, '') + '/' + p.replace(/[\\/]+/g, '/')
}

// ===== 状态徽章 =====
interface Badge { label: string; cls: string }
function badgesOf(sub: SubmoduleInfo): Badge[] {
  const out: Badge[] = []
  if (!sub.initialized) {
    out.push({ label: t('submodule.statusUninit'), cls: 'badge-muted' })
    return out
  }
  if (sub.modified) out.push({ label: t('submodule.statusModified'), cls: 'badge-warn' })
  if (sub.dirty) out.push({ label: t('submodule.statusDirty'), cls: 'badge-del' })
  if (out.length === 0) out.push({ label: t('submodule.statusOk'), cls: 'badge-ok' })
  return out
}

// ===== 每行操作菜单 =====
const menu = reactive<{ visible: boolean; x: number; y: number; sub: SubmoduleInfo | null }>({
  visible: false, x: 0, y: 0, sub: null,
})
function openMenu(e: MouseEvent, sub: SubmoduleInfo) {
  e.stopPropagation()
  menu.sub = sub
  menu.x = Math.min(e.clientX, window.innerWidth - 180)
  menu.y = Math.min(e.clientY, window.innerHeight - 220)
  menu.visible = true
  document.addEventListener('mousedown', closeMenuOutside)
  document.addEventListener('keydown', onMenuKey)
}
function closeMenu() {
  menu.visible = false
  menu.sub = null
  document.removeEventListener('mousedown', closeMenuOutside)
  document.removeEventListener('keydown', onMenuKey)
}
function closeMenuOutside(e: MouseEvent) {
  const el = document.getElementById('submodule-row-menu')
  if (el && !el.contains(e.target as Node)) closeMenu()
}
function onMenuKey(e: KeyboardEvent) {
  if (e.key === 'Escape') closeMenu()
}

// ===== 操作 =====
async function runWithBusy(fn: () => Promise<void>) {
  if (busy.value) return
  busy.value = true
  error.value = ''
  try {
    await fn()
    await loadList()
    emit('changed')
  } catch (e: any) {
    error.value = typeof e === 'string' ? e : e?.toString?.() || String(e)
  } finally {
    busy.value = false
  }
}

function onOpenSub(sub: SubmoduleInfo) {
  closeMenu()
  emit('open-submodule', absPath(sub.path))
}
function onUpdate(sub: SubmoduleInfo, remote: boolean) {
  closeMenu()
  runWithBusy(() => updateSubmodule(props.repoPath, sub.path, remote))
}
function onSync(sub: SubmoduleInfo) {
  closeMenu()
  runWithBusy(() => syncSubmodules(props.repoPath, false, sub.path))
}

// 移除确认
const removeTarget = ref<SubmoduleInfo | null>(null)
function askRemove(sub: SubmoduleInfo) {
  closeMenu()
  removeTarget.value = sub
}
async function confirmRemove() {
  if (!removeTarget.value) return
  const p = removeTarget.value.path
  removeTarget.value = null
  await runWithBusy(() => removeSubmodule(props.repoPath, p))
}

// 全局操作
function onUpdateAll() {
  runWithBusy(() => updateSubmodules(props.repoPath, false, false))
}
function onSyncAll() {
  runWithBusy(() => syncSubmodules(props.repoPath, false))
}

const showAdd = ref(false)
function onAdded() {
  showAdd.value = false
  loadList()
  emit('changed')
}

onBeforeUnmount(() => {
  document.removeEventListener('mousedown', closeMenuOutside)
  document.removeEventListener('keydown', onMenuKey)
})

const empty = computed(() => !loading.value && submodules.value.length === 0)
</script>

<template>
  <section class="submodule-panel" :class="{ collapsed }">
    <!-- 组头 -->
    <header class="sm-header" @click="collapsed = !collapsed">
      <button class="sm-collapse" :title="collapsed ? t('submodule.expand') : t('submodule.collapse')">
        <ChevronDown v-if="!collapsed" :size="15" />
        <ChevronRight v-else :size="15" />
      </button>
      <Package :size="15" class="sm-icon" />
      <span class="sm-title">{{ t('submodule.title') }}</span>
      <span v-if="submodules.length" class="sm-count">{{ submodules.length }}</span>
      <span class="sm-spacer" />

      <TooltipProvider v-if="!collapsed" :delay-duration="260">
        <div class="sm-actions" @click.stop>
          <Tooltip>
            <TooltipTrigger as-child>
              <button class="sm-btn" :disabled="busy" :title="t('submodule.add')" @click="showAdd = true">
                <Plus :size="15" />
              </button>
            </TooltipTrigger>
            <TooltipContent side="bottom">{{ t('submodule.add') }}</TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger as-child>
              <button class="sm-btn" :disabled="busy" :title="t('submodule.updateAll')" @click="onUpdateAll">
                <RefreshCw :size="15" />
              </button>
            </TooltipTrigger>
            <TooltipContent side="bottom">{{ t('submodule.updateAll') }}</TooltipContent>
          </Tooltip>
              <Tooltip>
                <TooltipTrigger as-child>
                  <button class="sm-btn" :disabled="busy" :title="t('submodule.syncAll')" @click="onSyncAll">
                    <Repeat :size="15" />
                  </button>
                </TooltipTrigger>
                <TooltipContent side="bottom">{{ t('submodule.syncAll') }}</TooltipContent>
              </Tooltip>
        </div>
      </TooltipProvider>
    </header>

    <!-- 列表 -->
    <div v-show="!collapsed" class="sm-body">
      <div v-if="error" class="sm-error">{{ error }}</div>

      <div v-if="loading" class="sm-empty">{{ t('common.loading') }}</div>

      <div v-else-if="empty" class="sm-empty">
        {{ t('submodule.empty') }}
      </div>

      <ul v-else class="sm-list">
        <li v-for="sub in submodules" :key="sub.path" class="sm-row">
          <div class="sm-row-main">
            <div class="sm-row-top">
              <span class="sm-name" :title="sub.path">{{ sub.name }}</span>
              <button class="sm-more" :title="t('submodule.actions')" @click="openMenu($event, sub)">
                <MoreVertical :size="15" />
              </button>
            </div>
            <div class="sm-row-sub">
              <span class="sm-url" :title="sub.url">{{ sub.url }}</span>
            </div>
            <div class="sm-row-meta">
              <code v-if="sub.head_commit_short" class="sm-commit">{{ sub.head_commit_short }}</code>
              <span v-else class="sm-commit sm-commit-empty">—</span>
              <span v-if="sub.branch" class="sm-branch">@{{ sub.branch }}</span>
              <span
                v-for="b in badgesOf(sub)"
                :key="b.label"
                class="sm-badge"
                :class="b.cls"
              >{{ b.label }}</span>
            </div>
          </div>
        </li>
      </ul>
    </div>

    <!-- 每行操作菜单 -->
    <div
      v-if="menu.visible && menu.sub"
      id="submodule-row-menu"
      class="sm-menu"
      :style="{ left: menu.x + 'px', top: menu.y + 'px' }"
    >
      <button class="sm-menu-item" @click="onOpenSub(menu.sub!)">
        <FolderOpen :size="15" /> {{ t('submodule.open') }}
      </button>
      <button class="sm-menu-item" @click="onUpdate(menu.sub!, false)">
        <Download :size="15" /> {{ t('submodule.update') }}
      </button>
      <button class="sm-menu-item" @click="onUpdate(menu.sub!, true)">
        <ArrowDownToLine :size="15" /> {{ t('submodule.updateRemote') }}
      </button>
      <button class="sm-menu-item" @click="onSync(menu.sub!)">
        <Repeat :size="15" /> {{ t('submodule.sync') }}
      </button>
      <div class="sm-menu-sep" />
      <button class="sm-menu-item sm-menu-danger" @click="askRemove(menu.sub!)">
        <Trash2 :size="15" /> {{ t('submodule.remove') }}
      </button>
    </div>

    <!-- 添加子模块弹窗 -->
    <AddSubmoduleDialog :open="showAdd" :repo-path="repoPath" @update:open="showAdd = $event" @added="onAdded" />

    <!-- 移除确认 -->
    <Dialog :open="!!removeTarget" @update:open="(v: boolean) => { if (!v) removeTarget = null }">
      <DialogContent class="sm:max-w-sm">
        <DialogHeader>
          <DialogTitle>{{ t('submodule.removeTitle') }}</DialogTitle>
          <DialogDescription>
            <AlertTriangle :size="15" class="sm-warn-icon" />
            {{ t('submodule.removeDesc', { path: removeTarget?.path || '' }) }}
          </DialogDescription>
        </DialogHeader>
        <DialogFooter class="!justify-end gap-2">
          <Button type="button" variant="outline" :disabled="busy" @click="removeTarget = null">
            {{ t('common.cancel') }}
          </Button>
          <Button type="button" variant="destructive" :disabled="busy" @click="confirmRemove">
            {{ busy ? t('common.loading') : t('common.delete') }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </section>
</template>

<style scoped>
.submodule-panel {
  display: flex;
  flex-direction: column;
  /* 内容驱动高度，最多占左栏 40%，超出部分内部滚动——
     不参与 branchPanelFlex 的 flex 分配，避免挤压仓库列表与分支面板 */
  flex: 0 0 auto;
  max-height: 40%;
  overflow: hidden;
  border-top: 1px solid var(--border-light, #e2e8f0);
  background: var(--bg-secondary, #f8fafc);
}
.submodule-panel.collapsed { max-height: none; }
.sm-header {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 7px 10px;
  cursor: pointer;
  user-select: none;
  flex: 0 0 auto;
}
.sm-collapse {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background: none;
  border: none;
  color: var(--text-muted, #64748b);
  cursor: pointer;
  padding: 0;
}
.sm-icon { color: var(--accent-primary, #3b82f6); }
.sm-title { font-size: 13px; font-weight: 600; color: var(--text-strong, #0f172a); }
.sm-count {
  font-size: 11px;
  color: var(--text-muted, #64748b);
  background: var(--bg-tertiary, #e2e8f0);
  border-radius: 10px;
  padding: 0 7px;
  line-height: 18px;
}
.sm-spacer { flex: 1 1 auto; }
.sm-actions { display: flex; gap: 2px; }
.sm-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  border-radius: 5px;
  border: none;
  background: none;
  color: var(--text-muted, #64748b);
  cursor: pointer;
}
.sm-btn:hover:not(:disabled) { background: var(--bg-tertiary, #e2e8f0); color: var(--text-strong, #0f172a); }
.sm-btn:disabled { opacity: 0.45; cursor: default; }

.sm-body { flex: 1 1 auto; min-height: 0; overflow-y: auto; padding: 2px 0 6px; }
.sm-empty { padding: 14px 12px; font-size: 12px; color: var(--text-muted, #64748b); }
.sm-error {
  margin: 6px 10px;
  font-size: 12px;
  color: var(--color-del, #ef4444);
  background: var(--bg-add, rgba(239, 68, 68, 0.06));
  padding: 6px 8px;
  border-radius: 4px;
  word-break: break-word;
}
.sm-list { list-style: none; margin: 0; padding: 0; }
.sm-row {
  padding: 6px 10px;
  border-bottom: 1px solid var(--border-light, #eef2f7);
  cursor: default;
}
.sm-row:hover { background: var(--bg-tertiary, #eef2f7); }
.sm-row-top { display: flex; align-items: center; gap: 6px; }
.sm-name { font-size: 13px; font-weight: 600; color: var(--text-strong, #0f172a); flex: 1 1 auto; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.sm-more {
  flex: 0 0 auto;
  display: inline-flex; align-items: center; justify-content: center;
  width: 22px; height: 22px; border-radius: 5px; border: none; background: none;
  color: var(--text-muted, #64748b); cursor: pointer;
}
.sm-more:hover { background: var(--bg-primary, #fff); color: var(--text-strong, #0f172a); }
.sm-row-sub { margin-top: 1px; }
.sm-url { font-size: 11px; color: var(--text-muted, #64748b); display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.sm-row-meta { margin-top: 4px; display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
.sm-commit { font-size: 11px; font-family: var(--font-mono, monospace); color: var(--text-muted, #475569); }
.sm-commit-empty { opacity: 0.5; }
.sm-branch { font-size: 11px; color: var(--text-muted, #64748b); }
.sm-badge {
  font-size: 10px;
  line-height: 16px;
  padding: 0 6px;
  border-radius: 8px;
  white-space: nowrap;
}
.badge-muted { color: var(--text-muted, #64748b); background: var(--bg-tertiary, #e2e8f0); }
.badge-ok { color: #16a34a; background: rgba(22, 163, 74, 0.1); }
.badge-warn { color: #d97706; background: rgba(217, 119, 6, 0.1); }
.badge-del { color: var(--color-del, #ef4444); background: var(--bg-add, rgba(239, 68, 68, 0.1)); }

.sm-menu {
  position: fixed;
  z-index: 1000;
  min-width: 160px;
  background: var(--bg-primary, #fff);
  border: 1px solid var(--border-light, #e2e8f0);
  border-radius: 8px;
  box-shadow: 0 8px 24px rgba(15, 23, 42, 0.18);
  padding: 4px;
}
.sm-menu-item {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  text-align: left;
  padding: 7px 10px;
  font-size: 13px;
  color: var(--text-strong, #0f172a);
  background: none;
  border: none;
  border-radius: 5px;
  cursor: pointer;
}
.sm-menu-item:hover { background: var(--bg-tertiary, #eef2f7); }
.sm-menu-danger { color: var(--color-del, #ef4444); }
.sm-menu-danger:hover { background: var(--bg-add, rgba(239, 68, 68, 0.08)); }
.sm-menu-sep { height: 1px; background: var(--border-light, #e2e8f0); margin: 4px 2px; }

.sm-warn-icon { display: inline; vertical-align: -2px; color: #d97706; margin-right: 4px; }
</style>
