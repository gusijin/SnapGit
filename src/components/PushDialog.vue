<script setup lang="ts">
import { ref, computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { getRemotes, getUpstream, pushChanges, saveCredentials, getRemoteUrl } from '../api/git'
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
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Card, CardContent } from '@/components/ui/card'

interface Props {
  repoPath: string
  branches: Branch[]
  currentBranch: string
  /** 预选推送的本地分支（默认取 currentBranch） */
  initialBranch?: string
}

const props = defineProps<Props>()
const emit = defineEmits(['close', 'pushed', 'pushing'])
const { t } = useI18n()

const remotes = ref<string[]>([])
const localBranch = ref('')
const remote = ref('')
const remoteBranch = ref('')
const isPushing = ref(false)
const error = ref('')
const loading = ref(true)

// 认证相关状态
const authMode = ref(false)
const username = ref('')
const token = ref('')
const remoteUrl = ref('')
const isSavingAuth = ref(false)

const localBranches = computed(() => props.branches.filter(b => !b.is_remote))

const selectedBranch = computed(() =>
  localBranches.value.find(b => b.name === localBranch.value) || null
)

async function init() {
  localBranch.value = props.initialBranch || props.currentBranch
  loading.value = true
  try {
    const [remoteList, upstream] = await Promise.all([
      getRemotes(props.repoPath),
      getUpstream(props.repoPath, localBranch.value),
    ])
    remotes.value = remoteList

    if (upstream) {
      remote.value = upstream.remote
      remoteBranch.value = upstream.remote_branch
    } else if (remoteList.length > 0) {
      remote.value = remoteList[0]
      remoteBranch.value = localBranch.value
    } else {
      remoteBranch.value = localBranch.value
    }
  } catch (e) {
    console.error('Load push info error:', e)
    remoteBranch.value = localBranch.value
  } finally {
    loading.value = false
  }
}

async function onLocalBranchChange() {
  if (!localBranch.value) return
  try {
    const upstream = await getUpstream(props.repoPath, localBranch.value)
    if (upstream) {
      remote.value = upstream.remote
      remoteBranch.value = upstream.remote_branch
    } else {
      remoteBranch.value = localBranch.value
    }
  } catch (e) {
    remoteBranch.value = localBranch.value
  }
}

function isAuthError(msg: string): boolean {
  const lower = msg.toLowerCase()
  return lower.includes('authentication failed')
    || lower.includes('access denied')
    || lower.includes('could not read username')
    || lower.includes('terminal prompts disabled')
    || lower.includes('认证失败')
}

async function handlePush() {
  if (!remote.value) {
    error.value = t('push.selectRemoteError')
    return
  }
  if (!remoteBranch.value.trim()) {
    error.value = t('push.remoteBranchError')
    return
  }
  error.value = ''
  isPushing.value = true
  emit('pushing', true)
  try {
    await pushChanges(
      props.repoPath,
      remote.value,
      localBranch.value,
      remoteBranch.value.trim(),
    )
    emit('pushed')
    emit('close')
  } catch (e) {
    const msg = String(e)
    error.value = msg
    if (isAuthError(msg)) {
      try {
        remoteUrl.value = await getRemoteUrl(props.repoPath, remote.value)
      } catch {}
      authMode.value = true
    }
  } finally {
    isPushing.value = false
    emit('pushing', false)
  }
}

async function handleSaveAuthAndPush() {
  if (!username.value.trim() || !token.value.trim()) {
    error.value = t('push.missingAuthError')
    return
  }
  error.value = ''
  isSavingAuth.value = true
  try {
    await saveCredentials(props.repoPath, remote.value, username.value.trim(), token.value.trim())
    authMode.value = false
    // 保存成功后重试推送
    await handlePush()
  } catch (e) {
    error.value = t('push.saveAuthError', { error: String(e) })
  } finally {
    isSavingAuth.value = false
  }
}

// 控制 Dialog 关闭：推送/保存凭证进行中禁止关闭
function onOpenChange(open: boolean) {
  if (!open) emit('close')
}

function guardClose(e: Event) {
  if (isPushing.value || isSavingAuth.value) e.preventDefault()
}

init()
</script>

<template>
  <Dialog :open="true" @update:open="onOpenChange">
    <DialogContent
      class="max-w-[420px] gap-3"
      @pointer-down-outside="guardClose"
      @escape-keydown="guardClose"
    >
      <DialogHeader>
        <DialogTitle>{{ t('push.title') }}</DialogTitle>
        <DialogDescription>{{ t('push.desc') }}</DialogDescription>
      </DialogHeader>

      <div class="grid gap-3 py-1">
        <div class="grid gap-1.5">
          <Label for="local-branch">{{ t('push.localBranch') }}</Label>
          <Select v-model="localBranch" :disabled="authMode" @update:model-value="onLocalBranchChange">
            <SelectTrigger id="local-branch">
              <SelectValue :placeholder="t('push.selectBranchPlaceholder')" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem v-for="b in localBranches" :key="b.name" :value="b.name">
                {{ b.name }}
              </SelectItem>
            </SelectContent>
          </Select>
        </div>

        <div v-if="selectedBranch && selectedBranch.ahead > 0" class="ahead-hint">
          {{ t('push.aheadPending', { n: selectedBranch.ahead }) }}
        </div>
        <div v-else-if="selectedBranch" class="ahead-hint muted">
          {{ t('push.noPending') }}
        </div>

        <div class="grid gap-1.5">
          <Label for="remote">{{ t('push.remote') }}</Label>
          <Select v-model="remote" :disabled="authMode">
            <SelectTrigger id="remote">
              <SelectValue :placeholder="t('push.selectRemotePlaceholder')" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem v-for="r in remotes" :key="r" :value="r">{{ r }}</SelectItem>
            </SelectContent>
          </Select>
        </div>

        <div class="grid gap-1.5">
          <Label for="remote-branch">{{ t('push.remoteBranch') }}</Label>
          <Input id="remote-branch" v-model="remoteBranch" :placeholder="t('push.remoteBranchPlaceholder')" :disabled="authMode" />
        </div>

        <div v-if="error" class="error-message">{{ error }}</div>

        <Card v-if="authMode" class="auth-card">
          <CardContent class="auth-card-body">
            <div class="auth-title">{{ t('push.authTitle') }}</div>
            <div v-if="remoteUrl" class="auth-url">{{ remoteUrl }}</div>
            <div class="grid gap-1.5">
              <Label for="username">{{ t('push.username') }}</Label>
              <Input id="username" v-model="username" :placeholder="t('push.usernamePlaceholder')" @keydown.enter="handleSaveAuthAndPush" />
            </div>
            <div class="grid gap-1.5">
              <Label for="token">{{ t('push.token') }}</Label>
              <Input id="token" v-model="token" type="password" :placeholder="t('push.tokenPlaceholder')" @keydown.enter="handleSaveAuthAndPush" />
            </div>
            <div class="auth-hint">
              {{ t('push.authHintLine1') }}<br />
              {{ t('push.authHintLine2') }}
            </div>
          </CardContent>
        </Card>
      </div>

      <DialogFooter>
        <Button variant="outline" @click="emit('close')" :disabled="isPushing || isSavingAuth">
          {{ t('push.cancel') }}
        </Button>
        <template v-if="authMode">
          <Button variant="secondary" @click="authMode = false; error = ''" :disabled="isSavingAuth">
            {{ t('push.back') }}
          </Button>
          <Button @click="handleSaveAuthAndPush" :disabled="isSavingAuth || !username.trim() || !token.trim()">
            {{ isSavingAuth ? t('push.saving') : t('push.saveAndPush') }}
          </Button>
        </template>
        <template v-else>
          <Button @click="handlePush" :disabled="isPushing || loading">
            {{ isPushing ? t('push.pushing') : t('push.pushBtn') }}
          </Button>
        </template>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>

<style scoped>
.ahead-hint {
  font-size: 11px;
  color: var(--color-add);
}

.ahead-hint.muted {
  color: var(--text-muted);
}

.error-message {
  font-size: 12px;
  color: var(--danger-color);
  background-color: var(--danger-bg);
  padding: 8px 10px;
  border-radius: 4px;
  word-break: break-all;
  white-space: pre-wrap;
}

.auth-card {
  border-color: var(--border-medium);
  background-color: var(--bg-tertiary);
}

.auth-card-body {
  padding: 12px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.auth-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-primary);
}

.auth-url {
  font-size: 11px;
  color: var(--text-muted);
  font-family: Consolas, Monaco, monospace;
  background-color: var(--bg-primary);
  padding: 6px 8px;
  border-radius: 4px;
  word-break: break-all;
}

.auth-hint {
  font-size: 11px;
  color: var(--text-muted);
  line-height: 1.5;
}
</style>
