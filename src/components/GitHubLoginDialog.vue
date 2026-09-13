<script setup lang="ts">
import { ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  startGitHubDeviceAuth,
  isGitHubClientIdConfigured,
  type GitHubAuthStatus,
} from '../githubAuth'
import { saveGitHubToken } from '../api/git'
import { open as shellOpen } from '@tauri-apps/plugin-shell'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Github, Loader2, Check, X, ExternalLink, AlertCircle } from 'lucide-vue-next'

const props = defineProps<{ open: boolean }>()
const emit = defineEmits(['update:open'])
const { t } = useI18n()

const status = ref<GitHubAuthStatus | null>(null)
const saving = ref(false)
const abort = ref<AbortController | null>(null)

function close() {
  abort.value?.abort()
  abort.value = null
  emit('update:open', false)
}

function openBrowser() {
  if (status.value?.type === 'waiting') {
    shellOpen(status.value.verificationUri).catch(() => {})
  }
}

async function start() {
  status.value = null
  saving.value = false
  if (!isGitHubClientIdConfigured()) {
    status.value = { type: 'error', message: t('githubLogin.notConfigured') }
    return
  }
  const controller = new AbortController()
  abort.value = controller
  startGitHubDeviceAuth((s) => {
    status.value = s
    if (s.type === 'authorized') {
      saving.value = true
      saveGitHubToken(s.token)
        .then(() => {
          saving.value = false
          // 成功反馈短暂停留后自动关闭
          setTimeout(() => close(), 1200)
        })
        .catch((e) => {
          saving.value = false
          status.value = { type: 'error', message: t('githubLogin.saveError', { error: String(e) }) }
        })
    }
  }, controller.signal)
}

function retry() {
  start()
}

// 打开即自动启动设备授权流；关闭（含取消）则中止轮询
watch(
  () => props.open,
  (v) => {
    if (v) start()
    else {
      abort.value?.abort()
      abort.value = null
    }
  },
)
</script>

<template>
  <Dialog :open="open" @update:open="(v: boolean) => !v && close()">
    <DialogContent class="github-login-dialog">
      <DialogHeader>
        <DialogTitle>
          <span class="gh-title">
            <Github :size="16" />
            {{ t('githubLogin.title') }}
          </span>
        </DialogTitle>
        <DialogDescription>{{ t('githubLogin.description') }}</DialogDescription>
      </DialogHeader>

      <!-- 未配置 Client ID：引导去设置 -->
      <div v-if="status?.type === 'error'" class="gh-state gh-error">
        <AlertCircle :size="18" />
        <span>{{ status.message }}</span>
      </div>

      <!-- 等待授权：展示验证码 + 打开浏览器 -->
      <div v-else-if="status?.type === 'waiting'" class="gh-state gh-waiting">
        <p class="gh-hint">{{ t('githubLogin.enterCode') }}</p>
        <div class="gh-code-box">
          <code>{{ status.userCode }}</code>
          <Button variant="outline" size="sm" class="gh-open-btn" @click="openBrowser">
            <ExternalLink :size="14" />
            {{ t('githubLogin.openBrowser') }}
          </Button>
        </div>
        <div class="gh-spinner-row">
          <Loader2 :size="14" class="gh-spin" />
          <span>{{ t('githubLogin.waiting') }}</span>
        </div>
      </div>

      <!-- 授权成功：保存凭证中 / 已完成 -->
      <div v-else-if="status?.type === 'authorized'" class="gh-state gh-ok">
        <Check :size="18" />
        <span>{{ saving ? t('githubLogin.saving') : t('githubLogin.success') }}</span>
      </div>

      <!-- 被拒绝 -->
      <div v-else-if="status?.type === 'denied'" class="gh-state gh-error">
        <AlertCircle :size="18" />
        <span>{{ t('githubLogin.denied') }}</span>
      </div>

      <!-- 验证码过期 -->
      <div v-else-if="status?.type === 'expired'" class="gh-state gh-error">
        <AlertCircle :size="18" />
        <span>{{ t('githubLogin.expired') }}</span>
      </div>

      <!-- 初始/兜底 -->
      <div v-else class="gh-state gh-muted">
        <Loader2 :size="14" class="gh-spin" />
        <span>{{ t('githubLogin.starting') }}</span>
      </div>

      <DialogFooter class="gh-footer">
        <template v-if="status?.type === 'waiting'">
          <Button variant="ghost" @click="close">
            <X :size="14" />
            {{ t('githubLogin.cancel') }}
          </Button>
        </template>
        <template v-else-if="status?.type === 'expired' || status?.type === 'denied'">
          <Button variant="outline" @click="retry">{{ t('githubLogin.retry') }}</Button>
          <Button variant="ghost" @click="close">{{ t('githubLogin.close') }}</Button>
        </template>
        <template v-else>
          <Button variant="ghost" @click="close">{{ t('githubLogin.close') }}</Button>
        </template>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>

<style scoped>
.github-login-dialog {
  max-width: 420px;
}

.gh-title {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  color: var(--text-primary);
}

.gh-state {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 4px 2px 8px;
  font-size: 13px;
  color: var(--text-secondary);
}

.gh-state.gh-error {
  color: var(--danger-color, #f44747);
  flex-direction: row;
  align-items: flex-start;
  gap: 8px;
}

.gh-state.gh-ok {
  color: var(--color-add, #4ec9b0);
  flex-direction: row;
  align-items: center;
  gap: 8px;
}

.gh-state.gh-muted {
  flex-direction: row;
  align-items: center;
  gap: 8px;
  color: var(--text-tertiary);
}

.gh-hint {
  margin: 0;
  line-height: 1.6;
}

.gh-code-box {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 12px 14px;
  background-color: var(--bg-tertiary);
  border: 1px solid var(--border-medium);
  border-radius: 8px;
}

.gh-code-box code {
  font-family: ui-monospace, 'SF Mono', SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 20px;
  font-weight: 700;
  letter-spacing: 2px;
  color: var(--text-bright);
}

.gh-open-btn {
  flex-shrink: 0;
}

.gh-spinner-row {
  display: flex;
  align-items: center;
  gap: 8px;
  color: var(--text-tertiary);
}

.gh-spin {
  animation: gh-spin 0.9s linear infinite;
}

@keyframes gh-spin {
  to { transform: rotate(360deg); }
}

.gh-footer {
  margin-top: 4px;
}
</style>
