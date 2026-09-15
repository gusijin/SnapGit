<script setup lang="ts">
import { ref, onMounted, computed, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  ChevronRight,
  User,
  GitBranch,
  Key,
  FileText,
  Github,
  Network,
  Link2,
  Terminal,
  Info,
  Check,
  AlertCircle,
  RefreshCw,
  CloudOff,
} from 'lucide-vue-next'
import { getRepoConfig, openFileDialog, setSshKeyPath, setUserIdentity } from '../api/git'
import type { RepoConfig, SshKeyInfo } from '../api/git'
import { getGitHubClientId, setGitHubClientId, isGitHubClientIdConfigured, isGitHubRemoteUrl } from '../githubAuth'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'

const props = defineProps<{
  open: boolean
  repoPath: string
}>()
const emit = defineEmits(['update:open'])
const { t } = useI18n()

function onOpenChange(v: boolean) { emit('update:open', v) }

const loading = ref(true)
const error = ref('')
const cfg = ref<RepoConfig | null>(null)
const showRaw = ref(false)

// 当前选中的 tab
type TabKey = 'basic' | 'remotes' | 'auth' | 'config'
const activeTab = ref<TabKey>('basic')

// 基本信息（user.name / user.email）编辑态
const editName = ref('')
const editEmail = ref('')
const savingIdentity = ref(false)
const identityMsg = ref('')
const identityErr = ref(false)

// SSH 私钥路径（id_rsa）编辑态
const editSshKeyPath = ref('')
const savingSshKey = ref(false)
const sshKeyMsg = ref('')
const sshKeyErr = ref(false)

// GitHub OAuth App Client ID（应用级，用于「跳转 GitHub 授权」推送）
const githubOAuthAppsUrl = 'https://github.com/settings/developers'
const editGitHubClientId = ref('')
const savingClientId = ref(false)
const clientIdMsg = ref('')
const clientIdErr = ref(false)
const githubClientIdConfigured = ref(false)

onMounted(async () => {
  if (!props.open) return
  await load()
})

watch(() => props.open, async (v) => {
  if (v) await load()
})

async function load() {
  loading.value = true
  error.value = ''
  if (!props.repoPath) {
    error.value = t('repoConfig.noRepo')
    loading.value = false
    return
  }
  try {
    cfg.value = await getRepoConfig(props.repoPath)
    editSshKeyPath.value = extractSshKeyPath(cfg.value?.core_ssh_command || null)
    editName.value = cfg.value?.user_name || ''
    editEmail.value = cfg.value?.user_email || ''
    sshKeyMsg.value = ''
    sshKeyErr.value = false
    identityMsg.value = ''
    identityErr.value = false
    // GitHub OAuth Client ID（应用级，回填已保存值；未配置则不显示占位常量）
    const id = getGitHubClientId()
    githubClientIdConfigured.value = isGitHubClientIdConfigured()
    editGitHubClientId.value = githubClientIdConfigured.value ? id : ''
    clientIdMsg.value = ''
    clientIdErr.value = false
  } catch (e) {
    error.value = String(e)
  } finally {
    loading.value = false
  }
}

// 从 core.sshCommand 中解析出 -i 指定的私钥路径（回填编辑框）
function extractSshKeyPath(cmd: string | null): string {
  if (!cmd) return ''
  const m = cmd.match(/-i\s+"([^"]+)"|-i\s+'([^']+)'|-i\s+(\S+)/)
  return toDisplayPath(m ? (m[1] || m[2] || m[3] || '') : '')
}

// Windows 下把路径统一显示为反斜杠；其它平台保留原样
function toDisplayPath(p: string | undefined): string {
  if (!p) return ''
  if (!navigator.userAgent.toLowerCase().includes('windows')) return p
  return p.replace(/[/\\]+/g, '\\')
}

// 保存基本信息（user.name / user.email）
async function onSaveIdentity() {
  savingIdentity.value = true
  identityMsg.value = ''
  identityErr.value = false
  try {
    await setUserIdentity(props.repoPath, editName.value, editEmail.value)
    await load()
    identityMsg.value = t('repoConfig.identitySaved')
  } catch (e) {
    identityMsg.value = String(e)
    identityErr.value = true
  } finally {
    savingIdentity.value = false
  }
}

async function onBrowseSshKey() {
  const p = await openFileDialog()
  if (p) editSshKeyPath.value = p
}

async function onSaveSshKey() {
  const path = editSshKeyPath.value.trim()
  savingSshKey.value = true
  sshKeyMsg.value = ''
  sshKeyErr.value = false
  try {
    await setSshKeyPath(props.repoPath, path)
    await load()
    sshKeyMsg.value = path ? t('repoConfig.saved') : t('repoConfig.cleared')
  } catch (e) {
    sshKeyMsg.value = String(e)
    sshKeyErr.value = true
  } finally {
    savingSshKey.value = false
  }
}

// 保存 GitHub OAuth App 的 Client ID（应用级、持久化到 localStorage）
async function onSaveGitHubClientId() {
  savingClientId.value = true
  clientIdMsg.value = ''
  clientIdErr.value = false
  try {
    setGitHubClientId(editGitHubClientId.value)
    githubClientIdConfigured.value = isGitHubClientIdConfigured()
    if (!githubClientIdConfigured.value) {
      editGitHubClientId.value = ''
      clientIdMsg.value = t('repoConfig.githubClientIdCleared')
    } else {
      clientIdMsg.value = t('repoConfig.githubClientIdSaved')
    }
  } catch (e) {
    clientIdMsg.value = String(e)
    clientIdErr.value = true
  } finally {
    savingClientId.value = false
  }
}

// OAuth Client ID 是 GitHub 专属的应用级配置：当前仓库的远端里没有 github.com 时不展示该块
const isGitHubRepo = computed(() =>
  (cfg.value?.remotes ?? []).some(
    rm => isGitHubRemoteUrl(rm.fetch_url) || isGitHubRemoteUrl(rm.push_url),
  ),
)

// 把 ssh_keys 按"私钥 + 公钥"两两配对
const sshKeyPairs = computed(() => {
  if (!cfg.value) return [] as { private?: SshKeyInfo; public?: SshKeyInfo }[]
  const result: { private?: SshKeyInfo; public?: SshKeyInfo }[] = []
  const privates = cfg.value.ssh_keys.filter(k => !k.path.endsWith('.pub'))
  for (const priv of privates) {
    const pub = cfg.value.ssh_keys.find(k => k.path === priv.path + '.pub')
    result.push({ private: priv, public: pub })
  }
  return result
})

// tab 定义（图标 + key），顺序即展示顺序
const tabs = computed(() => [
  { key: 'basic' as TabKey, label: t('repoConfig.tabBasic'), icon: User },
  { key: 'remotes' as TabKey, label: t('repoConfig.tabRemotes'), icon: GitBranch },
  { key: 'auth' as TabKey, label: t('repoConfig.tabAuth'), icon: Key },
  { key: 'config' as TabKey, label: t('repoConfig.tabConfig'), icon: FileText },
])
</script>

<template>
  <Dialog :open="open" @update:open="onOpenChange">
    <DialogContent class="repo-config-dialog">
      <DialogHeader>
        <DialogTitle>{{ t('repoConfig.title') }}</DialogTitle>
        <DialogDescription class="repo-path-desc">
          {{ repoPath }}
        </DialogDescription>
      </DialogHeader>

      <div v-if="loading" class="state-box">
        <RefreshCw :size="20" class="spin" />
        <span>{{ t('repoConfig.loading') }}</span>
      </div>
      <div v-else-if="error" class="state-box error">
        <AlertCircle :size="20" />
        <span>{{ error }}</span>
      </div>
      <div v-else-if="cfg" class="config-layout">

        <!-- 顶部 tab 栏（图标 + 文字） -->
        <div class="tab-bar">
          <button
            v-for="tab in tabs"
            :key="tab.key"
            class="tab-item"
            :class="{ active: activeTab === tab.key }"
            @click="activeTab = tab.key"
            type="button"
          >
            <component :is="tab.icon" :size="15" class="tab-icon" />
            <span class="tab-label">{{ tab.label }}</span>
          </button>
        </div>

        <!-- 内容区 -->
        <div class="tab-content">

          <!-- 基本信息 -->
          <section v-show="activeTab === 'basic'" class="cfg-section">
            <div class="group-title">
              <User :size="13" class="group-icon" />
              <span>{{ t('repoConfig.sectionIdentity') }}</span>
            </div>

            <div class="card">
              <div class="field-edit">
                <label class="field-label">
                  {{ t('repoConfig.identityName') }}
                  <code class="field-key">user.name</code>
                </label>
                <input
                  v-model="editName"
                  class="field-input"
                  :placeholder="t('repoConfig.notConfigured')"
                  spellcheck="false"
                  autocomplete="off"
                />
              </div>
              <div class="field-edit">
                <label class="field-label">
                  {{ t('repoConfig.identityEmail') }}
                  <code class="field-key">user.email</code>
                </label>
                <input
                  v-model="editEmail"
                  class="field-input"
                  :placeholder="t('repoConfig.notConfigured')"
                  spellcheck="false"
                  autocomplete="off"
                />
              </div>
              <div class="card-actions">
                <Button class="action-btn" @click="onSaveIdentity" :disabled="savingIdentity">
                  {{ t('repoConfig.save') }}
                </Button>
                <span v-if="identityMsg" class="msg" :class="{ error: identityErr }">
                  <Check v-if="!identityErr" :size="13" class="msg-icon" />
                  {{ identityMsg }}
                </span>
              </div>
            </div>

            <div class="hint">
              <Info :size="13" class="hint-icon" />
              <span>{{ t('repoConfig.identityHint') }}</span>
            </div>
          </section>

          <!-- 远程仓库 -->
          <section v-show="activeTab === 'remotes'" class="cfg-section">
            <div class="group-title">
              <GitBranch :size="13" class="group-icon" />
              <span>{{ t('repoConfig.sectionRemotes') }}</span>
            </div>

            <div v-if="cfg.remotes.length === 0" class="empty-state">
              <CloudOff :size="30" class="empty-icon" />
              <span>{{ t('repoConfig.noRemotes') }}</span>
            </div>

            <div v-for="rm in cfg.remotes" :key="rm.name" class="card remote-card">
              <div class="remote-head">
                <Network :size="15" class="remote-icon" />
                <span class="remote-name">{{ rm.name }}</span>
                <span class="remote-type-badge">{{
                  rm.fetch_url.startsWith('http') ? 'HTTPS' :
                  rm.fetch_url.startsWith('ssh') ? 'SSH' :
                  rm.fetch_url.startsWith('git@') ? 'SSH' : t('repoConfig.localType')
                }}</span>
              </div>
              <div class="url-row">
                <Link2 :size="13" class="url-icon" />
                <span class="url-label">{{ t('repoConfig.fetchUrl') }}</span>
                <code class="url-value">{{ rm.fetch_url || '—' }}</code>
              </div>
              <div v-if="rm.push_url && rm.push_url !== rm.fetch_url" class="url-row">
                <Link2 :size="13" class="url-icon" />
                <span class="url-label">{{ t('repoConfig.pushUrl') }}</span>
                <code class="url-value">{{ rm.push_url }}</code>
              </div>
            </div>
          </section>

          <!-- 认证与 SSH -->
          <section v-show="activeTab === 'auth'" class="cfg-section">
            <div class="group-title">
              <Key :size="13" class="group-icon" />
              <span>{{ t('repoConfig.authMethodsTitle') }}</span>
            </div>

            <!-- credential.helper -->
            <div class="field-inline">
              <span class="field-label">credential.helper</span>
              <code class="field-value mono">{{ cfg.credential_helper || t('repoConfig.defaultHelper') }}</code>
            </div>

            <!-- GitHub OAuth（应用级，用于「跳转 GitHub 授权」推送）；仅 github.com 远端才显示 -->
            <div v-if="isGitHubRepo" class="card gh-oauth-card">
              <div class="card-head">
                <span class="card-head-title">
                  <Github :size="14" class="head-icon" />
                  {{ t('repoConfig.githubClientId') }}
                </span>
                <span class="status-badge" :class="githubClientIdConfigured ? 'ok' : 'warn'">
                  {{ githubClientIdConfigured ? t('repoConfig.configuredBadge') : t('repoConfig.notConfiguredBadge') }}
                </span>
              </div>
              <div class="ssh-key-input-row">
                <input
                  v-model="editGitHubClientId"
                  class="ssh-key-input"
                  :placeholder="t('repoConfig.githubClientIdPlaceholder')"
                  spellcheck="false"
                  autocomplete="off"
                />
                <Button class="ssh-key-btn" @click="onSaveGitHubClientId" :disabled="savingClientId">
                  {{ t('repoConfig.save') }}
                </Button>
              </div>
              <div class="ssh-key-hint">
                {{ t('repoConfig.githubClientIdHint') }}
                <a :href="githubOAuthAppsUrl" target="_blank" rel="noopener" class="inline-link">{{ t('repoConfig.githubClientIdLink') }}</a>
              </div>
              <div v-if="clientIdMsg" class="ssh-key-msg" :class="{ error: clientIdErr }">
                <Check v-if="!clientIdErr" :size="13" class="msg-icon" />{{ clientIdMsg }}
              </div>
            </div>
            <div v-else class="info-box">
              <Info :size="15" class="info-icon" />
              <span>{{ t('repoConfig.githubNotApplicable') }}</span>
            </div>

            <!-- 本地 SSH 密钥 -->
            <div class="group-title sub">
              <Key :size="13" class="group-icon" />
              <span>{{ t('repoConfig.sshKeysTitle') }}</span>
            </div>

            <div class="card ssh-card">
              <div class="card-head">
                <span class="card-head-title">{{ t('repoConfig.sshKeyPath') }}</span>
              </div>
              <div class="ssh-key-input-row">
                <input
                  v-model="editSshKeyPath"
                  class="ssh-key-input"
                  :placeholder="t('repoConfig.sshKeyPlaceholder')"
                  spellcheck="false"
                  autocomplete="off"
                />
                <Button variant="outline" class="ssh-key-btn" @click="onBrowseSshKey" :disabled="savingSshKey">
                  {{ t('repoConfig.browse') }}
                </Button>
                <Button class="ssh-key-btn" @click="onSaveSshKey" :disabled="savingSshKey || !editSshKeyPath.trim()">
                  {{ t('repoConfig.save') }}
                </Button>
              </div>
              <div class="ssh-key-hint">{{ t('repoConfig.sshKeyHint') }}</div>
              <div v-if="sshKeyMsg" class="ssh-key-msg" :class="{ error: sshKeyErr }">{{ sshKeyMsg }}</div>
            </div>

            <div v-if="cfg.core_ssh_command || cfg.git_ssh_command_env || cfg.ssh_auth_sock_env" class="card env-card">
              <div class="card-head">
                <span class="card-head-title">
                  <Terminal :size="14" class="head-icon" />
                  {{ t('repoConfig.envTitle') }}
                </span>
              </div>
              <div v-if="cfg.core_ssh_command" class="field-inline">
                <span class="field-label">core.sshCommand</span>
                <code class="field-value mono">{{ cfg.core_ssh_command }}</code>
              </div>
              <div v-if="cfg.git_ssh_command_env" class="field-inline">
                <span class="field-label">GIT_SSH_COMMAND (env)</span>
                <code class="field-value mono">{{ cfg.git_ssh_command_env }}</code>
              </div>
              <div v-if="cfg.ssh_auth_sock_env" class="field-inline">
                <span class="field-label">SSH_AUTH_SOCK (env)</span>
                <code class="field-value mono">{{ cfg.ssh_auth_sock_env }}</code>
              </div>
            </div>

            <div v-if="sshKeyPairs.length" class="ssh-grid">
              <div v-for="pair in sshKeyPairs" :key="pair.private?.path" class="ssh-key-card">
                <div class="ssh-name">
                  <Key :size="13" class="ssh-name-icon" />
                  {{ pair.private ? pair.private.path.split(/[/\\]/).pop() : '' }}
                </div>
                <div class="ssh-status">
                  <span class="status-dot" :class="pair.private?.exists ? 'ok' : 'missing'" />
                  {{ pair.private?.exists ? t('repoConfig.exists') : t('repoConfig.missing') }}
                </div>
                <div v-if="pair.public" class="ssh-status subtle">
                  {{ t('repoConfig.publicKey') }}
                  <span class="status-dot" :class="pair.public.exists ? 'ok' : 'missing'" />
                  {{ pair.public.exists ? t('repoConfig.exists') : t('repoConfig.missing') }}
                </div>
                <div class="ssh-path" :title="pair.private?.path">{{ toDisplayPath(pair.private?.path) }}</div>
              </div>
            </div>
          </section>

          <!-- 本地完整 Git Config -->
          <section v-show="activeTab === 'config'" class="cfg-section raw-section">
            <button class="raw-toggle" @click="showRaw = !showRaw" type="button">
              <ChevronRight :size="12" class="chevron" :class="{ open: showRaw }" />
              {{ t('repoConfig.rawConfig', { n: Object.keys(cfg.local_config).length }) }}
            </button>
            <div v-if="showRaw" class="raw-list">
              <div v-for="(v, k) in cfg.local_config" :key="k" class="raw-row">
                <code class="raw-key">{{ k }}</code>
                <span class="raw-eq">=</span>
                <code class="raw-val">{{ v }}</code>
              </div>
              <div v-if="Object.keys(cfg.local_config).length === 0" class="empty-tip">
                {{ t('repoConfig.noRawItems') }}
              </div>
            </div>
          </section>

        </div>
      </div>

      <div class="dialog-footer">
        <Button variant="outline" @click="load" :disabled="loading">{{ t('repoConfig.refresh') }}</Button>
        <Button variant="outline" @click="onOpenChange(false)">{{ t('repoConfig.close') }}</Button>
      </div>
    </DialogContent>
  </Dialog>
</template>

<script lang="ts">
export default { name: 'RepoConfigDialog' }
</script>

<style scoped>
.repo-config-dialog {
  /* 覆盖 ui/dialog 的 max-w-lg(512px)：左栏 128 + gap 16 + 右侧内容，总宽给足 900px */
  max-width: 900px;
  max-height: 85vh;
  overflow-y: auto;
}

.repo-path-desc {
  font-family: Consolas, Monaco, monospace;
  font-size: 11.5px;
  word-break: break-all;
}

/* 加载 / 错误 状态 */
.state-box {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 28px 0;
  color: var(--text-tertiary);
  font-size: 13px;
}
.state-box.error {
  color: var(--danger-color);
}
.spin {
  animation: spin 1s linear infinite;
}
@keyframes spin {
  to { transform: rotate(360deg); }
}

/* tab 布局：顶部横排 tab + 下方内容 */
.config-layout {
  display: flex;
  flex-direction: column;
  gap: 14px;
  padding-top: 4px;
}
.tab-bar {
  display: flex;
  gap: 2px;
  border-bottom: 1px solid var(--border-medium);
  padding-bottom: 6px;
  flex-wrap: nowrap;
  overflow-x: auto;
  scrollbar-width: none;
}
.tab-bar::-webkit-scrollbar {
  display: none;
}
.tab-item {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 5px 10px;
  font-size: 12px;
  border: 1px solid transparent;
  border-radius: 7px;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  white-space: nowrap;
  flex-shrink: 0;
  transition: background 0.12s ease, color 0.12s ease, border-color 0.12s ease;
}
.tab-item:hover {
  background: var(--bg-tertiary);
}
.tab-item.active {
  background: var(--brand-bg);
  color: var(--brand-primary);
  font-weight: 600;
  border-color: var(--brand-bg);
}
.tab-icon {
  flex-shrink: 0;
}
.tab-content {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.cfg-section {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.empty-tip {
  font-size: 12px;
  color: var(--text-tertiary);
  padding: 8px 0;
}

/* 分组标题（带 lucide 图标的轻量小标题，呼应面板标题规范） */
.group-title {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  color: var(--text-tertiary);
  padding-top: 2px;
}
.group-title.sub {
  margin-top: 2px;
}
.group-icon {
  color: var(--brand-primary);
  flex-shrink: 0;
}

/* 通用卡片 */
.card {
  border: 1px solid var(--border-medium);
  border-radius: 8px;
  background: var(--bg-secondary);
  padding: 12px 14px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.card-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}
.card-head-title {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 12.5px;
  font-weight: 600;
  color: var(--text-primary);
}
.head-icon {
  color: var(--brand-primary);
}

/* 基本信息编辑行 */
.field-edit {
  display: flex;
  flex-direction: column;
  gap: 5px;
}
.field-label {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  color: var(--text-tertiary);
  font-size: 12px;
}
.field-key {
  font-family: Consolas, Monaco, monospace;
  font-size: 10.5px;
  color: var(--text-muted);
  background: var(--bg-tertiary);
  padding: 0 5px;
  border-radius: 4px;
}
.field-input {
  width: 100%;
  min-width: 0;
  padding: 6px 9px;
  font-size: 12px;
  color: var(--text-primary);
  background: var(--bg-primary);
  border: 1px solid var(--border-medium);
  border-radius: 6px;
  outline: none;
}
.field-input:focus {
  border-color: var(--brand-primary);
  box-shadow: 0 0 0 2px var(--brand-bg);
}
.card-actions {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 2px;
}
.action-btn {
  flex-shrink: 0;
}

/* 提示 / 消息 */
.hint,
.ssh-key-hint {
  display: flex;
  align-items: flex-start;
  gap: 6px;
  font-size: 11px;
  color: var(--text-tertiary);
  line-height: 1.5;
}
.hint-icon {
  color: var(--brand-primary);
  flex-shrink: 0;
  margin-top: 1px;
}
.msg,
.ssh-key-msg {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  font-size: 11.5px;
  color: var(--color-add, #22c55e);
}
.msg.error,
.ssh-key-msg.error {
  color: var(--danger-color, #ef4444);
}
.msg-icon {
  flex-shrink: 0;
}

/* 行内字段（credential.helper / env 注入） */
.field-inline {
  display: flex;
  align-items: baseline;
  gap: 8px;
  font-size: 12.5px;
  flex-wrap: wrap;
}
.field-inline .field-label {
  color: var(--text-tertiary);
  flex-shrink: 0;
}
.field-value {
  color: var(--text-primary);
  word-break: break-all;
}
.field-value.mono {
  font-family: Consolas, Monaco, monospace;
}

/* 远程卡片 */
.remote-card {
  gap: 8px;
}
.remote-head {
  display: flex;
  align-items: center;
  gap: 8px;
}
.remote-icon {
  color: var(--brand-primary);
  flex-shrink: 0;
}
.remote-name {
  font-weight: 600;
  font-size: 13px;
  color: var(--text-primary);
}
.remote-type-badge {
  font-size: 10.5px;
  padding: 1px 8px;
  border-radius: 10px;
  background: var(--brand-bg);
  color: var(--brand-primary);
  font-weight: 500;
}
.url-row {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
}
.url-icon {
  color: var(--text-muted);
  flex-shrink: 0;
}
.url-label {
  color: var(--text-tertiary);
  flex-shrink: 0;
  min-width: 56px;
}
.url-value {
  color: var(--text-primary);
  word-break: break-all;
  font-family: Consolas, Monaco, monospace;
  font-size: 11.5px;
}

/* 空状态 */
.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  padding: 30px 0;
  color: var(--text-tertiary);
  font-size: 12.5px;
}
.empty-icon {
  opacity: 0.5;
}

/* GitHub OAuth / SSH 私钥 输入框行 */
.ssh-key-input-row {
  display: flex;
  gap: 6px;
  align-items: center;
  flex-wrap: wrap;
}
.ssh-key-input {
  flex: 1;
  min-width: 0;
  padding: 6px 9px;
  font-size: 12px;
  font-family: Consolas, Monaco, monospace;
  color: var(--text-primary);
  background: var(--bg-primary);
  border: 1px solid var(--border-medium);
  border-radius: 6px;
  outline: none;
}
.ssh-key-input:focus {
  border-color: var(--brand-primary);
  box-shadow: 0 0 0 2px var(--brand-bg);
}
.ssh-key-btn {
  flex-shrink: 0;
}
.inline-link {
  color: var(--brand-primary);
  text-decoration: none;
  border-bottom: 1px solid var(--brand-primary);
  cursor: pointer;
}
.inline-link:hover {
  opacity: 0.8;
}

/* 状态徽章（标题右侧：已配置 / 未配置） */
.status-badge {
  font-size: 10px;
  font-weight: 600;
  padding: 2px 8px;
  border-radius: 8px;
  white-space: nowrap;
}
.status-badge.ok {
  background: var(--bg-add);
  color: var(--color-add);
}
.status-badge.warn {
  background: var(--bg-mod);
  color: var(--color-mod);
}

/* info 提示框（非 github 源时替代 OAuth 配置块） */
.info-box {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  padding: 10px 12px;
  border-radius: 8px;
  background: var(--bg-tertiary);
  border: 1px dashed var(--border-medium);
  font-size: 12px;
  color: var(--text-secondary);
  line-height: 1.5;
}
.info-icon {
  color: var(--accent-primary);
  flex-shrink: 0;
  margin-top: 1px;
}

/* SSH 密钥网格 */
.ssh-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 8px;
}
@media (max-width: 520px) {
  .ssh-grid { grid-template-columns: 1fr; }
}
.ssh-key-card {
  border: 1px solid var(--border-medium);
  border-radius: 8px;
  padding: 10px 12px;
  display: flex;
  flex-direction: column;
  gap: 3px;
  background: var(--bg-secondary);
}
.ssh-name {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-weight: 600;
  font-size: 12.5px;
  color: var(--text-primary);
}
.ssh-name-icon {
  color: var(--brand-primary);
  flex-shrink: 0;
}
.ssh-status {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: var(--text-secondary);
}
.ssh-status.subtle {
  font-size: 11.5px;
  color: var(--text-tertiary);
}
.status-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  flex-shrink: 0;
}
.status-dot.ok {
  background: var(--color-add, #22c55e);
  box-shadow: 0 0 0 2px var(--bg-add, #22c55e22);
}
.status-dot.missing {
  background: var(--danger-color, #ef4444);
  box-shadow: 0 0 0 2px var(--danger-bg, #ef444422);
}
.ssh-path {
  font-family: Consolas, Monaco, monospace;
  font-size: 11px;
  color: var(--text-tertiary);
  word-break: break-all;
}

/* Raw config */
.raw-section {
  margin-top: 4px;
}
.raw-toggle {
  background: none;
  border: none;
  color: var(--text-secondary);
  font-size: 12.5px;
  cursor: pointer;
  padding: 4px 0;
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
.raw-toggle:hover {
  color: var(--brand-primary);
}
/* 展开箭头：lucide ChevronRight（不再依赖 emoji 的 font-size），展开时旋转 90° */
.chevron {
  transition: transform 0.15s ease;
  display: inline-block;
  flex-shrink: 0;
}
.chevron.open {
  transform: rotate(90deg);
}
.raw-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 8px 10px;
  border-radius: 6px;
  background: var(--bg-secondary);
  border: 1px dashed var(--border-medium);
  max-height: 220px;
  overflow-y: auto;
}
.raw-row {
  font-family: Consolas, Monaco, monospace;
  font-size: 11.5px;
  display: flex;
  gap: 4px;
  line-height: 1.5;
  flex-wrap: wrap;
}
.raw-key {
  color: var(--brand-primary);
  flex-shrink: 0;
}
.raw-eq {
  color: var(--text-tertiary);
}
.raw-val {
  color: var(--text-primary);
  word-break: break-all;
}

.dialog-footer {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  padding-top: 8px;
}
</style>
