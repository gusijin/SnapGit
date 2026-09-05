<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { Languages } from 'lucide-vue-next'
import { availableLocales, setLocale } from '../i18n'

const { locale } = useI18n()

const open = defineModel<boolean>('open', { default: false })

function select(code: string) {
  setLocale(code)
  open.value = false
}
</script>

<template>
  <div class="lang-toggle" @click.stop="open = !open">
    <Languages :size="15" />
    <!-- 纯文本下拉（复用菜单栏样式风格），保持轻量无需额外依赖 -->
    <div v-if="open" class="lang-popover" @click.stop>
      <div
        v-for="lang in availableLocales()"
        :key="lang.code"
        class="lang-item"
        :class="{ active: lang.code === locale }"
        @click.stop="select(lang.code)"
      >
        <span class="lang-label">{{ lang.label }}</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.lang-toggle {
  position: relative;
  width: 30px;
  height: 30px;
  margin-right: 2px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 8px;
  cursor: pointer;
  color: var(--text-secondary);
  transition: background-color 0.15s ease, color 0.15s ease;
}

.lang-toggle:hover {
  background-color: var(--bg-hover);
  color: var(--text-bright);
}

.lang-popover {
  position: absolute;
  top: calc(100% + 6px);
  right: 0;
  min-width: 130px;
  background-color: var(--bg-secondary);
  border: 1px solid var(--border-medium);
  border-radius: 10px;
  padding: 5px;
  box-shadow: 0 10px 30px rgba(2, 6, 23, 0.35);
  z-index: 1001;
  animation: submenu-in 0.14s ease;
}

.lang-item {
  display: flex;
  align-items: center;
  padding: 6px 10px;
  border-radius: 6px;
  cursor: pointer;
  font-size: 13px;
  color: var(--text-secondary);
  transition: background-color 0.12s ease, color 0.12s ease;
}

.lang-item:hover {
  background-color: var(--brand-bg);
  color: var(--text-bright);
}

.lang-item.active {
  color: var(--brand-primary);
  font-weight: 500;
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
</style>