<script setup lang="ts">
import { ref, onMounted, onBeforeUnmount } from 'vue'
import type { Component } from 'vue'
import { useI18n } from 'vue-i18n'
import { useTheme } from '../stores/theme'
import { availableLocales, setLocale } from '../i18n'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import LanguageSwitcher from './LanguageSwitcher.vue'
import {
  FolderOpen,
  Download,
  LogOut,
  GitCommitVertical,
  ArrowUpFromLine,
  ArrowDownToLine,
  RefreshCw,
  GitBranch,
  ArrowLeftRight,
  Settings,
  Palette,
  Info,
  Sun,
  Moon,
  Check,
} from 'lucide-vue-next'

const emit = defineEmits([
  'open-repo', 'clone-repo', 'commit', 'push', 'pull',
  'branch', 'checkout-branch', 'merge', 'refresh',
  'repo-config', 'quit', 'about', 'toggle-theme'
])

const { theme, toggleTheme } = useTheme()

const { t, locale } = useI18n()

interface SubMenuItem {
  labelKey: string
  action: string
  icon?: Component
}
interface SubMenuSeparator {
  type: 'separator'
}
interface SubMenuLabel {
  type: 'label'
  labelKey: string
}
interface SubMenuLocale {
  type: 'locale'
  code: string
  label: string
}
type SubMenuEntry = SubMenuItem | SubMenuSeparator | SubMenuLabel | SubMenuLocale

interface MenuEntry {
  nameKey: string
  submenu: SubMenuEntry[]
}

const languageList = availableLocales()

const menuItems: MenuEntry[] = [
  {
    nameKey: 'menu.file',
    submenu: [
      { labelKey: 'menu.openRepo', action: 'open-repo', icon: FolderOpen },
      { labelKey: 'menu.cloneRepo', action: 'clone-repo', icon: Download },
      { type: 'separator' },
      { labelKey: 'menu.quit', action: 'quit', icon: LogOut }
    ]
  },
  {
    nameKey: 'menu.edit',
    submenu: [
      { labelKey: 'menu.config', action: 'repo-config', icon: Settings },
      { type: 'separator' },
      { type: 'label', labelKey: 'menu.language' },
      ...languageList.map((l) => ({ type: 'locale' as const, code: l.code, label: l.label }))
    ]
  },
  {
    nameKey: 'menu.repository',
    submenu: [
      { labelKey: 'menu.commit', action: 'commit', icon: GitCommitVertical },
      { labelKey: 'menu.push', action: 'push', icon: ArrowUpFromLine },
      { labelKey: 'menu.pull', action: 'pull', icon: ArrowDownToLine },
      { type: 'separator' },
      { labelKey: 'menu.refresh', action: 'refresh', icon: RefreshCw }
    ]
  },
  {
    nameKey: 'menu.branch',
    submenu: [
      { labelKey: 'menu.newBranch', action: 'branch', icon: GitBranch },
      { labelKey: 'menu.checkoutBranch', action: 'checkout-branch', icon: ArrowLeftRight }
    ]
  },
  {
    nameKey: 'menu.view',
    submenu: [
      { type: 'separator' },
      { labelKey: 'menu.toggleTheme', action: 'toggle-theme', icon: Palette }
    ]
  },
  {
    nameKey: 'menu.help',
    submenu: [
      { labelKey: 'menu.aboutSnapGit', action: 'about', icon: Info }
    ]
  }
]

const activeMenu = ref<string | null>(null)
const showAbout = ref(false)
const menubarRef = ref<HTMLElement | null>(null)
// 悬停离开后的延迟收起缓冲：给鼠标水平扫过切换到下一个菜单留时间，避免闪断
let menuLeaveTimer: ReturnType<typeof setTimeout> | null = null

function toggleMenu(name: string) {
  clearMenuLeaveTimer()
  activeMenu.value = activeMenu.value === name ? null : name
}

function closeMenu() {
  clearMenuLeaveTimer()
  activeMenu.value = null
}

function clearMenuLeaveTimer() {
  if (menuLeaveTimer) {
    clearTimeout(menuLeaveTimer)
    menuLeaveTimer = null
  }
}

// 悬停进入：若当前已有下拉打开，则切换到此菜单
function onMenuEnter(name: string) {
  clearMenuLeaveTimer()
  if (activeMenu.value !== null) {
    activeMenu.value = name
  }
}

// 悬停离开（菜单项 + 其下拉整体）：延迟收起，期间进入下一菜单则取消
function onMenuLeave(name: string) {
  if (activeMenu.value !== name) return
  clearMenuLeaveTimer()
  menuLeaveTimer = setTimeout(() => {
    menuLeaveTimer = null
    if (activeMenu.value === name) {
      activeMenu.value = null
    }
  }, 150)
}

// 仅在点击菜单栏（含其下拉）以外的区域时关闭，避免鼠标移向子菜单途中误收起
function onDocClick(e: MouseEvent) {
  if (menubarRef.value && !menubarRef.value.contains(e.target as Node)) {
    closeMenu()
  }
}

onMounted(() => document.addEventListener('click', onDocClick))
onBeforeUnmount(() => {
  document.removeEventListener('click', onDocClick)
  clearMenuLeaveTimer()
})

async function handleAction(action: string | undefined) {
  if (!action) return
  closeMenu()

  // 优先处理可以直接在组件内完成的动作
  if (action === 'toggle-theme') {
    toggleTheme()
    emit('toggle-theme')
    return
  }
  if (action === 'about') {
    showAbout.value = true
    emit('about')
    return
  }
  if (action === 'quit') {
    try {
      const { getCurrentWindow } = await import('@tauri-apps/api/window')
      await getCurrentWindow().close()
    } catch {
      window.close()
    }
    emit('quit')
    return
  }
  if (action === 'repo-config') {
    emit('repo-config')
    return
  }

  // 其他动作通过 emit 交给父组件处理
  emit(action as any)
}
</script>

<template>
  <div class="menubar" ref="menubarRef" @click="closeMenu">

    <div
      v-for="menu in menuItems"
      :key="menu.nameKey"
      class="menu"
      :class="{ active: activeMenu === menu.nameKey }"
      @click.stop="toggleMenu(menu.nameKey)"
      @mouseenter="onMenuEnter(menu.nameKey)"
      @mouseleave="onMenuLeave(menu.nameKey)"
    >
      <span class="menu-label">{{ t(menu.nameKey) }}</span>
      <div v-if="activeMenu === menu.nameKey" class="submenu" @click.stop>
        <template v-for="(item, idx) in menu.submenu" :key="idx">
          <div v-if="(item as any).type === 'separator'" class="separator"></div>
          <div v-else-if="(item as any).type === 'label'" class="submenu-label">
            {{ t((item as any).labelKey) }}
          </div>
          <div
            v-else-if="(item as any).type === 'locale'"
            class="submenu-item"
            :class="{ active: locale === (item as any).code }"
            @click.stop="setLocale((item as any).code); closeMenu()"
          >
            <Check v-if="locale === (item as any).code" :size="16" class="item-icon" />
            <span v-else class="item-icon-placeholder" />
            <span class="item-label">{{ (item as any).label }}</span>
          </div>
          <div
            v-else
            class="submenu-item"
            @click.stop="handleAction((item as any).action)"
          >
            <component :is="(item as any).icon" :size="16" class="item-icon" />
            <span class="item-label">{{ t((item as any).labelKey) }}</span>
          </div>
        </template>
      </div>
    </div>

    <div class="menubar-actions">
      <LanguageSwitcher />
      <div
        class="theme-toggle"
        @click.stop="toggleTheme"
        :title="theme === 'dark' ? t('menu.toggleTheme') : t('menu.toggleTheme')"
      >
        <component :is="theme === 'dark' ? Sun : Moon" :size="15" />
      </div>
    </div>
  </div>

  <Dialog v-model:open="showAbout">
    <DialogContent class="sm:max-w-md">
      <DialogHeader>
        <DialogTitle class="flex items-center gap-2">
          <span class="inline-flex h-7 w-7 items-center justify-center rounded-lg bg-primary text-primary-foreground font-bold">S</span>
          SnapGit
        </DialogTitle>
        <DialogDescription>
          {{ t('about.supportedBy') }}
        </DialogDescription>
      </DialogHeader>

      <div class="space-y-3 text-sm">
        <div class="flex items-center justify-between">
          <span class="text-muted-foreground">{{ t('about.version') }}</span>
          <span class="font-medium">v0.1.0</span>
        </div>
        <div class="flex items-center justify-between gap-4">
          <span class="text-muted-foreground">{{ t('about.techStack') }}</span>
          <span class="font-medium text-right">Tauri 2 · Vue 3 · TypeScript</span>
        </div>
        <p class="text-muted-foreground leading-relaxed">
          {{ t('about.description') }}
        </p>
      </div>

      <DialogFooter>
        <Button variant="default" @click="showAbout = false">{{ t('about.confirm') }}</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>

<style scoped>
.menubar {
  display: flex;
  align-items: center;
  height: 34px;
  flex-shrink: 0;
  background-color: var(--bg-tertiary);
  border-bottom: 1px solid var(--border-color);
  -webkit-user-select: none;
  -webkit-user-drag: none;
  user-select: none;
  padding-left: 8px;
  padding-right: 4px;
}

/* 顶层菜单项 */
.menu {
  position: relative;
  padding: 0 11px;
  cursor: pointer;
  display: flex;
  align-items: center;
  height: 100%;
  font-size: 12.5px;
  color: var(--text-secondary);
  transition: background-color 0.12s ease, color 0.12s ease;
}

.menu:hover {
  background-color: var(--bg-hover);
  color: var(--text-bright);
}

.menu.active {
  background-color: var(--brand-bg);
  color: var(--brand-primary);
  font-weight: 500;
}

.menu-label {
  line-height: 1;
}

/* 下拉子菜单：与菜单栏无缝紧贴（top:100% 无间隙），圆角卡片 + 柔和阴影 + 淡入 */
.submenu {
  position: absolute;
  top: 100%;
  left: 0;
  min-width: 208px;
  background-color: var(--bg-secondary);
  border: 1px solid var(--border-medium);
  border-radius: 0 0 10px 10px;
  padding: 5px 0;
  box-shadow: 0 10px 30px rgba(2, 6, 23, 0.35);
  z-index: 1000;
  animation: submenu-in 0.14s ease;
}

@keyframes submenu-in {
  from {
    opacity: 0;
    transform: translateY(-4px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}

.submenu-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 7px 12px;
  margin: 1px 5px;
  border-radius: 6px;
  cursor: pointer;
  font-size: 13px;
  color: var(--text-secondary);
  transition: background-color 0.12s ease, color 0.12s ease;
}

.submenu-item:hover {
  background-color: var(--brand-bg);
  color: var(--text-bright);
}

.item-icon {
  flex-shrink: 0;
  color: var(--text-tertiary);
  transition: color 0.12s ease;
}

.submenu-item:hover .item-icon {
  color: var(--accent-text);
}

.item-label {
  line-height: 1;
}

.separator {
  height: 1px;
  background-color: var(--border-light);
  margin: 5px 12px;
}

/* 子菜单分组标题（如「语言」） */
.submenu-label {
  padding: 5px 12px 3px;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  color: var(--text-tertiary);
}

/* 当前选中语言项：高亮 + 左侧图标占位保持对齐 */
.submenu-item.active {
  color: var(--brand-primary);
  font-weight: 500;
}

.submenu-item.active .item-icon {
  color: var(--brand-primary);
}

.item-icon-placeholder {
  display: inline-block;
  width: 16px;
  flex-shrink: 0;
}

/* 右侧动作区（语言切换 + 主题切换） */
.menubar-actions {
  margin-left: auto;
  display: flex;
  align-items: center;
  height: 100%;
}

/* 主题切换按钮 */
.theme-toggle {
  width: 30px;
  height: 30px;
  margin-right: 4px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 8px;
  cursor: pointer;
  color: var(--text-secondary);
  transition: background-color 0.15s ease, color 0.15s ease;
}

.theme-toggle:hover {
  background-color: var(--bg-hover);
  color: var(--text-bright);
}
</style>
