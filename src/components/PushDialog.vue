<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { Key, TriangleAlert } from 'lucide-vue-next'
import { getRemotes, getUpstream, pushChanges, pullWithStrategy, saveCredentials, getRemoteUrl, getRepoConfig } from '../api/git'
import { classifyPushError } from '../pushError'
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
  /** 打开时预置的推送失败信息（用于提交并推送因「非快进」被拒时，直接展示「拉取并推送」出口） */
  initialError?: string
}

const props = defineProps<Props>()
const emit = defineEmits(['close', 'pushed', 'pushing', 'pull-conflict'])
const { t } = useI18n()

const remotes = ref<string[]>([])
const localBranch = ref('')
const remote = ref('')
const remoteBranch = ref('')
const isPushing = ref(false)
const isPulling = ref(false)
const error = ref('')
// 推送被拒（非快进）：远程有本地没有的新提交 → 展示「拉取并推送」出口
const rejected = ref(false)
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
  // 由「提交并推送」因非快进被拒而打开：预置错误信息，直接展示「拉取并推送」出口
  if (props.initialError) {
    const info = classifyPushError(props.initialError)
    error.value = info.detail
    rejected.value = info.kind === 'non-ff'
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
    const info = classifyPushError(String(e))
    error.value = info.detail
    // 推送被拒（非快进）：远程有本地没有的新提交 → 展示「拉取并推送」出口
    rejected.value = info.kind === 'non-ff'
    // 已配置 SSH 私钥：失败后不自动跳转「远程仓库认证」界面，保持普通推送界面（推送按钮始终可用），
    // 仅展示错误诊断；SSH 私钥与访问令牌二选一即可，不应在已配 SSH 时强求令牌。
    if (!rejected.value && !sshConfigured.value && (info.kind === 'credential' || info.kind === 'ssh')) {
      await loadRemoteUrl()
      authMode.value = true
    }
  } finally {
    isPushing.value = false
    emit('pushing', false)
  }
}

/**
 * 推送被拒（非快进）时的一键修复：先以 merge 方式拉取远程新提交，成功后再重试推送。
 * - 拉取成功 → 清除被拒状态并重试推送；
 * - 拉取产生冲突 → 关闭本弹窗并把冲突交给主窗口「变更文件」面板解决（emit pull-conflict）。
 */
async function onPullAndPush() {
  if (isPulling.value || isPushing.value) return
  isPulling.value = true
  try {
    await pullWithStrategy(props.repoPath, 'merge')
    rejected.value = false
    isPulling.value = false
    await handlePush()
  } catch (e) {
    const msg = typeof e === 'string' ? e : (e?.toString?.() || String(e))
    if (msg.startsWith('PULL_CONFLICT:')) {
      emit('pull-conflict', 'merge')
      emit('close')
      return
    }
    error.value = classifyPushError(msg).detail
    rejected.value = false
  } finally {
    isPulling.value = false
  }
}

// 手动切到认证模式（已配置 SSH 但确需令牌时的备选入口；默认不进入）
async function enterAuthMode() {
  await loadRemoteUrl()
  authMode.value = true
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
  if (isPushing.value || isPulling.value || isSavingAuth.value || ghActive.value) e.preventDefault()
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

        <!-- 推送被拒（非快进）：友好说明 + 可折叠原始错误（原始错误默认收起，避免噪声） -->
        <div v-if="rejected" class="rejected-box">
          <div class="rejected-title">
            <TriangleAlert :size="13" />
            <span>{{ t('push.rejectedTitle') }}</span>
          </div>
          <p class="rejected-desc">
            {{ t('push.rejectedDesc', { remote, branch: remoteBranch.trim() || localBranch }) }}
          </p>
          <details v-if="error" class="rejected-detail">
            <summary>{{ t('push.rejectedDetail') }}</summary>
            <pre>{{ error }}</pre>
          </details>
        </div>
        <div v-else-if="error" class="error-message">{{ error }}</div>
        <!-- 已配置 SSH 私钥但推送失败时的备选入口：默认不进入认证界面，需用户主动切换 -->
        <div v-if="error && !rejected && sshConfigured && !authMode" class="auth-fallback">
          <button type="button" class="link-btn" @click="enterAuthMode">
            {{ t('push.useTokenInstead') }}
          </button>
        </div>

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
        <Button variant="outline" @click="emit('close')" :disabled="isPushing || isPulling || isSavingAuth">
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
          <!-- 被拒时提供两个出口：直接重试推送 / 一键「拉取并推送」 -->
          <Button v-if="rejected" variant="outline" @click="handlePush" :disabled="isPushing || isPulling">
            {{ t('push.pushBtn') }}
          </Button>
          <Button v-if="rejected" @click="onPullAndPush" :disabled="isPushing || isPulling">
            {{ isPulling ? t('push.pulling') : t('push.pullAndPush') }}
          </Button>
          <Button v-else @click="handlePush" :disabled="isPushing || loading">
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

/* 推送被拒（非快进）：温和的警示块；主操作是「拉取并推送」，原始错误默认折叠 */
.rejected-box {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 8px 10px;
  border-radius: 6px;
  border: 1px solid var(--border-medium);
  background-color: var(--danger-bg);
}

.rejected-title {
  display: flex;
  align-items: center;
  gap: 5px;
  font-size: 12px;
  font-weight: 600;
  color: var(--danger-color);
}

.rejected-desc {
  margin: 0;
  font-size: 12px;
  line-height: 1.55;
  color: var(--text-secondary);
  word-break: break-word;
}

.rejected-detail summary {
  font-size: 11.5px;
  color: var(--text-tertiary);
  cursor: pointer;
  user-select: none;
}

.rejected-detail pre {
  margin: 6px 0 0;
  max-height: 160px;
  overflow: auto;
  font-size: 11px;
  line-height: 1.5;
  color: var(--text-secondary);
  font-family: Consolas, Monaco, monospace;
  white-space: pre-wrap;
  word-break: break-all;
}

/* 已配 SSH 但失败时的备选入口（低调文字链接） */
.auth-fallback {
  display: flex;
  justify-content: flex-end;
}
.link-btn {
  background: none;
  border: none;
  padding: 0;
  font-size: 11.5px;
  color: var(--brand-primary);
  cursor: pointer;
  text-decoration: underline;
  text-underline-offset: 2px;
}
.link-btn:hover {
  opacity: 0.8;
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
