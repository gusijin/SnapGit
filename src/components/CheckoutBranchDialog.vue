<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import type { Branch } from '../types'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { GitBranch, ArrowRight, Search } from 'lucide-vue-next'

const props = defineProps<{
  open: boolean
  branches: Branch[]
  currentBranch: string
}>()
const emit = defineEmits(['update:open', 'checkout'])
const { t } = useI18n()

function onOpenChange(v: boolean) { emit('update:open', v) }

const keyword = ref('')
const hoverIdx = ref(0)

const localBranches = computed(() =>
  props.branches.filter(b => !b.is_remote)
)

const filteredBranches = computed(() => {
  const kw = keyword.value.trim().toLowerCase()
  if (!kw) return localBranches.value
  return localBranches.value.filter(b => b.name.toLowerCase().includes(kw))
})

watch(() => props.open, (v) => {
  if (v) {
    keyword.value = ''
    hoverIdx.value = 0
  }
})

watch(filteredBranches, (list) => {
  if (hoverIdx.value >= list.length) hoverIdx.value = 0
})

function confirmCheckout(branch?: Branch) {
  const target = branch || filteredBranches.value[hoverIdx.value]
  if (!target) return
  if (target.name === props.currentBranch) {
    emit('update:open', false)
    return
  }
  emit('checkout', target.name)
  emit('update:open', false)
}

function onKeyDown(e: KeyboardEvent) {
  if (e.key === 'ArrowDown') {
    e.preventDefault()
    hoverIdx.value = Math.min(hoverIdx.value + 1, filteredBranches.value.length - 1)
  } else if (e.key === 'ArrowUp') {
    e.preventDefault()
    hoverIdx.value = Math.max(hoverIdx.value - 1, 0)
  } else if (e.key === 'Enter') {
    e.preventDefault()
    confirmCheckout()
  }
}
</script>

<template>
  <Dialog :open="open" @update:open="onOpenChange">
    <DialogContent class="checkout-dialog">
      <DialogHeader>
        <DialogTitle>{{ t('checkout.title') }}</DialogTitle>
        <DialogDescription>
          {{ t('checkout.desc') }}
        </DialogDescription>
      </DialogHeader>

      <div class="search-row" @keydown="onKeyDown">
        <Search :size="14" class="search-icon" />
        <Input
          v-model="keyword"
          :placeholder="t('checkout.searchPlaceholder')"
          autocomplete="off"
          spellcheck="false"
          autofocus
        />
      </div>

      <div class="branch-list">
        <div
          v-for="(branch, idx) in filteredBranches"
          :key="branch.name"
          class="branch-item"
          :class="{
            active: idx === hoverIdx,
            current: branch.is_current,
          }"
          @mouseenter="hoverIdx = idx"
          @click="confirmCheckout(branch)"
        >
          <GitBranch :size="14" class="branch-icon" />
          <span class="branch-name">{{ branch.name }}</span>
          <span v-if="branch.is_current" class="branch-badge current">{{ t('checkout.current') }}</span>
          <span
            v-else-if="branch.ahead || branch.behind"
            class="branch-badge"
            :class="{ ahead: branch.ahead, behind: branch.behind }"
          >
            <span v-if="branch.ahead">+{{ branch.ahead }}</span>
            <span v-if="branch.ahead && branch.behind">·</span>
            <span v-if="branch.behind">-{{ branch.behind }}</span>
          </span>
          <ArrowRight
            v-if="idx === hoverIdx && !branch.is_current"
            :size="13"
            class="branch-arrow"
          />
        </div>
        <div v-if="filteredBranches.length === 0" class="empty-tip">
          {{ t('checkout.noMatch') }}
        </div>
      </div>

      <DialogFooter>
        <Button variant="outline" @click="onOpenChange(false)">{{ t('checkout.cancel') }}</Button>
        <Button
          @click="confirmCheckout()"
          :disabled="filteredBranches.length === 0 || filteredBranches[hoverIdx]?.name === currentBranch"
        >
          {{ t('checkout.switchTo', { branch: filteredBranches[hoverIdx]?.name || '...' }) }}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>

<style scoped>
.checkout-dialog {
  max-width: 440px;
}

.search-row {
  position: relative;
  margin-bottom: 2px;
}
.search-icon {
  position: absolute;
  left: 10px;
  top: 50%;
  transform: translateY(-50%);
  color: var(--text-tertiary);
  pointer-events: none;
}
.search-row Input {
  padding-left: 30px;
}

.branch-list {
  max-height: 300px;
  overflow-y: auto;
  border: 1px solid var(--border-medium);
  border-radius: 8px;
  background: var(--bg-secondary);
}

.branch-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  cursor: pointer;
  font-size: 13px;
  color: var(--text-secondary);
  transition: background-color 0.1s ease;
  border-bottom: 1px solid var(--border-light);
}
.branch-item:last-child {
  border-bottom: none;
}
.branch-item:hover,
.branch-item.active {
  background: var(--brand-bg);
  color: var(--text-primary);
}
.branch-item.current {
  color: var(--text-tertiary);
  font-style: italic;
  cursor: default;
}
.branch-item.current:hover,
.branch-item.current.active {
  background: transparent;
  color: var(--text-tertiary);
}

.branch-icon {
  color: var(--text-tertiary);
  flex-shrink: 0;
}
.branch-item.active .branch-icon {
  color: var(--brand-primary);
}

.branch-name {
  flex: 1;
  font-family: Consolas, Monaco, monospace;
}

.branch-badge {
  font-size: 10.5px;
  padding: 1px 7px;
  border-radius: 10px;
  font-weight: 500;
  flex-shrink: 0;
}
.branch-badge.current {
  background: var(--bg-tertiary);
  color: var(--text-tertiary);
  font-style: normal;
}
.branch-badge.ahead {
  background: rgba(234, 179, 8, 0.15);
  color: #ca8a04;
}
.branch-badge.behind {
  background: rgba(239, 68, 68, 0.15);
  color: #dc2626;
}

.branch-arrow {
  color: var(--brand-primary);
  flex-shrink: 0;
}

.empty-tip {
  padding: 20px;
  text-align: center;
  color: var(--text-tertiary);
  font-size: 12.5px;
}
</style>
