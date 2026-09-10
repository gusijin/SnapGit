<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { AlertTriangle } from 'lucide-vue-next'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'

const { t } = useI18n()

defineProps<{ open: boolean }>()
const emit = defineEmits<{
  (e: 'update:open', v: boolean): void
  (e: 'force-exit'): void
}>()

function cancel() {
  emit('update:open', false)
}

function forceExit() {
  emit('update:open', false)
  emit('force-exit')
}
</script>

<template>
  <Dialog :open="open" @update:open="(v: boolean) => !v && cancel()">
    <DialogContent class="sm:max-w-[420px]">
      <DialogHeader>
        <DialogTitle class="flex items-center gap-2">
          <AlertTriangle class="h-5 w-5 text-[var(--danger-color,#ef4444)]" />
          {{ t('git.closeConfirm.title') }}
        </DialogTitle>
        <DialogDescription class="text-base font-medium text-[var(--text-primary)]">
          {{ t('git.closeConfirm.question') }}
        </DialogDescription>
      </DialogHeader>

      <p class="text-sm text-[var(--text-secondary)]">
        {{ t('git.closeConfirm.hint') }}
      </p>

      <DialogFooter class="gap-2 sm:justify-end">
        <Button variant="outline" class="text-[var(--danger-color,#ef4444)]" @click="forceExit">
          {{ t('git.closeConfirm.forceExit') }}
        </Button>
        <Button @click="cancel">
          {{ t('common.cancel') }}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
