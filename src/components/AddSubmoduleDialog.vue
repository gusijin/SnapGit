<script setup lang="ts">
import { ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { addSubmodule } from '../api/git'
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
import { Label } from '@/components/ui/label'

const props = defineProps<{
  open: boolean
  repoPath: string
}>()
const emit = defineEmits<{
  (e: 'update:open', value: boolean): void
  (e: 'added'): void
}>()

const { t } = useI18n()

const url = ref('')
const path = ref('')
const submitting = ref(false)
const error = ref('')

function onOpenChange(v: boolean) { emit('update:open', v) }

// 弹窗打开时清空上一轮的提交状态（防止错误/输入残留干扰）
watch(() => props.open, (v) => {
  if (v) {
    error.value = ''
    submitting.value = false
    url.value = ''
    path.value = ''
  }
})

async function submit() {
  const u = url.value.trim()
  if (!u) {
    error.value = t('submodule.urlRequired')
    return
  }
  submitting.value = true
  error.value = ''
  try {
    await addSubmodule(props.repoPath, u, path.value.trim())
    emit('update:open', false)
    emit('added')
  } catch (e: any) {
    error.value = typeof e === 'string' ? e : e?.toString?.() || String(e)
  } finally {
    submitting.value = false
  }
}
</script>

<template>
  <Dialog :open="open" @update:open="onOpenChange">
    <DialogContent class="sm:max-w-md">
      <DialogHeader>
        <DialogTitle>{{ t('submodule.addTitle') }}</DialogTitle>
        <DialogDescription>{{ t('submodule.addDesc') }}</DialogDescription>
      </DialogHeader>

      <form class="form-grid" @submit.prevent="submit">
        <div class="form-row">
          <Label for="sub-url">{{ t('submodule.url') }}</Label>
          <Input
            id="sub-url"
            v-model="url"
            :placeholder="'https://github.com/owner/repo.git'"
            autocomplete="off"
            spellcheck="false"
            :disabled="submitting"
          />
        </div>
        <div class="form-row">
          <Label for="sub-path">{{ t('submodule.path') }}</Label>
          <Input
            id="sub-path"
            v-model="path"
            :placeholder="t('submodule.pathPlaceholder')"
            autocomplete="off"
            spellcheck="false"
            :disabled="submitting"
          />
          <span class="form-hint">{{ t('submodule.pathHint') }}</span>
        </div>

        <div v-if="error" class="form-error">{{ error }}</div>
      </form>

      <DialogFooter class="!justify-end gap-2">
        <Button type="button" variant="outline" :disabled="submitting" @click="onOpenChange(false)">
          {{ t('common.cancel') }}
        </Button>
        <Button type="button" :disabled="submitting" @click="submit">
          {{ submitting ? t('common.loading') : t('submodule.addSubmit') }}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>

<style scoped>
.form-grid {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 4px 0 8px;
}
.form-row {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.form-hint {
  font-size: 11px;
  color: var(--text-muted, #64748b);
}
.form-error {
  font-size: 12px;
  color: var(--color-del, #ef4444);
  background: var(--bg-add, rgba(239, 68, 68, 0.06));
  padding: 6px 8px;
  border-radius: 4px;
  word-break: break-word;
}
</style>
