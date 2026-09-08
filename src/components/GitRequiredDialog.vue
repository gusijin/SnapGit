<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { AlertTriangle, Download, RefreshCw } from 'lucide-vue-next'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { useGit } from '@/stores/git'

const { t } = useI18n()
const { showGitRequired, openGitDownload, retryDetect, gitOs, detecting } = useGit()
</script>

<template>
  <Dialog v-model:open="showGitRequired">
    <DialogContent class="sm:max-w-[460px]">
      <DialogHeader>
        <DialogTitle class="flex items-center gap-2">
          <AlertTriangle class="h-5 w-5 text-[var(--danger-color,#ef4444)]" />
          {{ t('git.required.title') }}
        </DialogTitle>
        <DialogDescription>{{ t('git.required.desc') }}</DialogDescription>
      </DialogHeader>

      <div class="space-y-3 py-1 text-sm text-[var(--text-secondary)]">
        <p v-if="gitOs === 'macos'">{{ t('git.required.macHint') }}</p>
        <p v-else>{{ t('git.required.winHint') }}</p>

        <div
          v-if="gitOs === 'macos'"
          class="rounded-md bg-[var(--bg-tertiary)] px-3 py-2 font-mono text-xs text-[var(--text-primary)]"
        >
          xcode-select --install
        </div>
      </div>

      <DialogFooter class="gap-2 sm:justify-between">
        <Button variant="outline" :disabled="detecting" @click="retryDetect">
          <RefreshCw class="mr-1 h-4 w-4" :class="{ 'animate-spin': detecting }" />
          {{ t('git.required.retry') }}
        </Button>
        <Button @click="openGitDownload">
          <Download class="mr-1 h-4 w-4" />
          {{ t('git.required.download') }}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
