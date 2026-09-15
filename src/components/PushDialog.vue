<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { Key } from 'lucide-vue-next'
import { getRemotes, getUpstream, pushChanges, saveCredentials, getRemoteUrl, getRepoConfig } from '../api/git'
import { startGitHubDeviceAuth, isGitHubClientIdConfigured, isGitHubRemoteUrl, type GitHubAuthStatus } from '../githubAuth'
import { open } from '@tauri-apps/plugin-shell'
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
  /** 打开即进入认证模式（用于提交并推送时认证失败，直接展示授权入口） */
  initialAuthMode?: boolean
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

// GitHub 设备授权流（Device Flow）：点按钮 → 跳浏览器授权 → 自动拿 token
const ghActive = ref(false)
const ghStatus = ref<GitHubAuthStatus | null>(null)
const ghAbort = ref<AbortController | null>(null)

// 是否已配置 SSH 私钥（core.sshCommand 非空）：配了则优先走 SSH，无需访问令牌
const sshConfigured = ref(false)

const localBranches = computed(() => props.branches.filter(b => !b.is_remote))

const selectedBranch = computed(() =>
  localBranches.value.find(b => b.name === localBranch.value) || null
)

/** 当前远端是不是 GitHub：只有 github.com 及其子域才给「GitHub 授权」入口 */
const isGitHubRemote = computed(() => isGitHubRemoteUrl(remoteUrl.value))

/** 拉取当前选中远端的 URL（用于展示 + 判定是否 GitHub） */
async function loadRemoteUrl() {
  if (!remote.value) return
  try {
    remoteUrl.value = await getRemoteUrl(props.repoPath, remote.value)
  } catch {
    remoteUrl.value = ''
  }
}

// 切换远端后旧 URL 失效，清空以免误判成 GitHub
watch(remote, () => {
  remoteUrl.value = ''
})

async function init() {
  localBranch.value = props.initialBranch || props.currentBranch
  // 认证失败后直接打开对话框时，预置认证模式（远程 URL 要等远端确定后再取）
  if (props.initialAuthMode) authMode.value = true
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
  // 检测当前仓库是否已配置 SSH 私钥（core.sshCommand），用于认证失败时分流提示
  try {
    const cfg = await getRepoConfig(props.repoPath)
    sshConfigured.value = !!(cfg && cfg.core_ssh_command)
  } catch {
    sshConfigured.value = false
  }
  // 远端已确定，取 URL 用于展示并判定是否 GitHub
  if (authMode.value) await loadRemoteUrl()
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

// 凭证类失败（HTTPS 场景）：需用户名/令牌或 credential helper
function isCredentialError(msg: string): boolean {
  const lower = msg.toLowerCase()
  return lower.includes('authentication failed')
    || lower.includes('access denied')
    || lower.includes('could not read username')
    || lower.includes('terminal prompts disabled')
    || lower.includes('认证失败')
}

// SSH 公钥类失败：已配 SSH 私钥但未通过认证，应提示检查 SSH 而非强求令牌
function isSshAuthError(msg: string): boolean {
  const lower = msg.toLowerCase()
  return lower.includes('permission denied (publickey)')
    || lower.includes('could not read from remote repository')
    || lower.includes('ssh 公钥认证未通过')
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
    // 凭证失败 或 SSH 公钥失败 都进入认证区，但提示文案分流（SSH 已配则优先 SSH 诊断，令牌为备选）
    if (isCredentialError(msg) || isSshAuthError(msg)) {
      await loadRemoteUrl()
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

// GitHub 设备授权流：启动 → 轮询 → 成功后存凭证并重试推送
function startGithubAuth() {
  // 非 GitHub 远端（GitLab / Gitee / 自建等）不给设备授权入口
  if (ghActive.value || !isGitHubRemote.value) return
  if (!isGitHubClientIdConfigured()) {
    ghStatus.value = { type: 'notconfigured' }
    return
  }
  ghActive.value = true
  ghStatus.value = null
  error.value = ''
  const controller = new AbortController()
  ghAbort.value = controller
  startGitHubDeviceAuth((s) => {
    ghStatus.value = s
    if (s.type === 'authorized') {
      // GitHub 约定 OAuth/PAT 用户名为 x-access-token，存好凭证后直接重试推送
      saveCredentials(props.repoPath, remote.value, 'x-access-token', s.token)
        .then(() => {
          ghActive.value = false
          authMode.value = false
          return handlePush()
        })
        .catch((e) => {
          ghActive.value = false
          error.value = t('push.saveAuthError', { error: String(e) })
        })
    }
  }, controller.signal)
}

function cancelGithubAuth() {
  ghAbort.value?.abort()
  ghAbort.value = null
  ghActive.value = false
}

async function openGhPage() {
  if (ghStatus.value?.type === 'waiting') {
    try {
      await open(ghStatus.value.verificationUri)
    } catch {}
  }
}

// 控制 Dialog 关闭：推送/保存凭证/授权进行中禁止关闭
function onOpenChange(open: boolean) {
  if (!open) {
    cancelGithubAuth()
    emit('close')
  }
}

function guardClose(e: Event) {
  if (isPushing.value || isSavingAuth.value || ghActive.value) e.preventDefault()
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

            <!-- 已配置 SSH 私钥：优先使用 SSH 认证，无需访问令牌；令牌仅作备选 -->
            <div v-if="sshConfigured" class="ssh-note">
              <Key :size="13" class="ssh-note-icon" />
              <span>{{ t('push.sshConfiguredHint') }}</span>
            </div>

            <!-- GitHub 设备授权流（推荐）：仅当远端是 github.com 时才显示 -->
            <div v-if="isGitHubRemote" class="gh-auth">
              <Button class="gh-auth-btn" @click="startGithubAuth" :disabled="ghActive">
                {{ ghActive ? t('push.githubAuthAuthorizing') : t('push.githubAuthBtn') }}
              </Button>
              <div v-if="ghStatus && ghStatus.type === 'notconfigured'" class="gh-warn">{{ t('push.githubAuthNotConfigured') }}<span class="gh-config-hint">（仓库设置 → 认证与 SSH）</span></div>

              <div v-if="ghStatus && ghStatus.type === 'waiting'" class="gh-waiting">
                <div class="gh-code-label">{{ t('push.githubAuthCodeLabel') }}</div>
                <div class="gh-code">{{ ghStatus.userCode }}</div>
                <div class="gh-waiting-hint">{{ t('push.githubAuthWaiting') }}</div>
                <Button variant="link" class="gh-open" @click="openGhPage">{{ t('push.githubAuthOpenPage') }}</Button>
              </div>
              <div v-else-if="ghStatus && ghStatus.type === 'authorized'" class="gh-ok">{{ t('push.githubAuthSuccess') }}</div>
              <div v-else-if="ghStatus && ghStatus.type === 'denied'" class="gh-err">{{ t('push.githubAuthDenied') }}</div>
              <div v-else-if="ghStatus && ghStatus.type === 'expired'" class="gh-err">{{ t('push.githubAuthExpired') }}</div>
              <div v-else-if="ghStatus && ghStatus.type === 'error'" class="gh-err">{{ t('push.githubAuthError', { error: ghStatus.message }) }}</div>
            </div>

            <div v-if="isGitHubRemote" class="auth-divider"><span>{{ t('push.orUseToken') }}</span></div>

            <!-- 手动 token（兜底） -->
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

/* 已配置 SSH 私钥时的提示：优先 SSH，令牌为备选 */
.ssh-note {
  display: flex;
  align-items: flex-start;
  gap: 6px;
  padding: 8px 10px;
  border-radius: 6px;
  background-color: var(--bg-add, #22c55e18);
  border: 1px solid var(--border-light);
  font-size: 11.5px;
  line-height: 1.5;
  color: var(--text-secondary);
}
.ssh-note-icon {
  color: var(--color-add, #22c55e);
  flex-shrink: 0;
  margin-top: 1px;
}

/* GitHub 设备授权流面板 */
.gh-auth {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.gh-auth-btn {
  width: 100%;
}

.gh-warn {
  font-size: 11px;
  color: var(--danger-color);
  line-height: 1.5;
}

.gh-waiting {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 10px;
  border: 1px dashed var(--border-medium);
  border-radius: 6px;
  background-color: var(--bg-primary);
}

.gh-code-label {
  font-size: 11px;
  color: var(--text-muted);
}

.gh-code {
  font-size: 22px;
  font-weight: 700;
  letter-spacing: 0.12em;
  color: var(--brand-primary);
  font-family: Consolas, Monaco, monospace;
  user-select: all;
}

.gh-waiting-hint {
  font-size: 11px;
  color: var(--text-muted);
  line-height: 1.5;
}

.gh-open {
  align-self: flex-start;
  height: auto;
  padding: 0;
  font-size: 12px;
}

.gh-ok {
  font-size: 12px;
  color: var(--color-add);
}

.gh-err {
  font-size: 12px;
  color: var(--danger-color);
  word-break: break-word;
}

.auth-divider {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 4px 0;
  color: var(--text-muted);
  font-size: 11px;
}

.auth-divider::before,
.auth-divider::after {
  content: '';
  flex: 1;
  height: 1px;
  background-color: var(--border-light);
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
