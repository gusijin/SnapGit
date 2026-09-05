<script setup lang="ts">
import { ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { addRemote } from '../api/git'
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

// 默认名固定为 origin，符合绝大多数仓库（GitHub/GitLab/自建）的惯例。
// 用户可在输入框里改成 "upstream" 等，但允许空：业务层兜底为 origin。
const name = ref('origin')
const url = ref('')
const submitting = ref(false)
const error = ref('')

function onOpenChange(v: boolean) { emit('update:open', v) }

// 弹窗打开时清空上一轮的提交状态（防止错误/URL 残留干扰）
watch(() => props.open, (v) => {
  if (v) {
    error.value = ''
    submitting.value = false
    // 不主动清 name（保留 origin 默认值），URL 清空让用户重新填
    url.value = ''
  }
})

async function submit() {
  const n = name.value.trim() || 'origin'
  const u = url.value.trim()
  if (!u) {
    error.value = t('repository.remoteUrlRequired')
    return
  }
  submitting.value = true
  error.value = ''
  try {
    await addRemote(props.repoPath, n, u)
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
        <DialogTitle>{{ t('repository.addRemoteTitle') }}</DialogTitle>
        <DialogDescription>{{ t('repository.addRemoteDesc') }}</DialogDescription>
      </DialogHeader>

      <form class="form-grid" @submit.prevent="submit">
        <div class="form-row">
          <Label for="remote-name">{{ t('repository.remoteName') }}</Label>
          <Input
            id="remote-name"
            v-model="name"
            :placeholder="'origin'"
            autocomplete="off"
            spellcheck="false"
            :disabled="submitting"
          />
        </div>
        <div class="form-row">
          <Label for="remote-url">{{ t('repository.remoteUrl') }}</Label>
          <Input
            id="remote-url"
            v-model="url"
            :placeholder="'https://github.com/owner/repo.git'"
            autocomplete="off"
            spellcheck="false"
            :disabled="submitting"
          />
        </div>

        <div v-if="error" class="form-error">{{ error }}</div>
      </form>

      <DialogFooter class="!justify-end gap-2">
        <Button type="button" variant="outline" :disabled="submitting" @click="onOpenChange(false)">
          {{ t('common.cancel') }}
        </Button>
        <Button type="button" :disabled="submitting" @click="submit">
          {{ submitting ? t('common.loading') : t('repository.addRemoteSubmit') }}
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
.form-error {
  font-size: 12px;
  color: var(--color-del, #ef4444);
  background: var(--bg-add, rgba(239, 68, 68, 0.06));
  /* 增加一点点内边距保证易读，且不使用 emoji */
  padding: 6px 8px;
  border-radius: 4px;
  /* 与色板对齐：仅文字色变红 + 软底色，不抢视觉重心 */
  word-break: break-word;
}
</style>
