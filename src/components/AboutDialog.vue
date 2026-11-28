<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'

// 两端共用：Windows/Linux 由 MenuBar 的「关于」emit 打开，macOS 由原生菜单
// 的 menu-action('about') 打开。放在 Repository.vue 里，macOS 不渲染 MenuBar 也能用。
const props = defineProps<{ open: boolean }>()
const emit = defineEmits<{ 'update:open': [value: boolean] }>()
const { t } = useI18n()

function onOpenChange(value: boolean) {
  emit('update:open', value)
}
</script>

<template>
  <Dialog :open="props.open" @update:open="onOpenChange">
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
          <span class="font-medium">v1.0.2</span>
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
        <Button variant="default" @click="onOpenChange(false)">{{ t('about.confirm') }}</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
