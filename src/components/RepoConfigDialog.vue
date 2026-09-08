<script setup lang="ts">
import { ref, onMounted, computed, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { ChevronRight } from 'lucide-vue-next'
import { getRepoConfig, openFileDialog, setSshKeyPath } from '../api/git'
import type { RepoConfig, SshKeyInfo } from '../api/git'
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

// SSH 私钥路径（id_rsa）编辑态
const editSshKeyPath = ref('')
const savingSshKey = ref(false)
const sshKeyMsg = ref('')
const sshKeyErr = ref(false)

onMounted(async () => {
  if (!props.open || !props.repoPath) return
  await load()
})

watch(() => props.open, async (v) => {
  if (v && props.repoPath) await load()
})

async function load() {
  loading.value = true
  error.value = ''
  try {
    cfg.value = await getRepoConfig(props.repoPath)
    editSshKeyPath.value = extractSshKeyPath(cfg.value?.core_ssh_command || null)
    sshKeyMsg.value = ''
    sshKeyErr.value = false
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
  return m ? (m[1] || m[2] || m[3] || '') : ''
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

      <div v-if="loading" class="loading">{{ t('repoConfig.loading') }}</div>
      <div v-else-if="error" class="error-message">{{ error }}</div>
      <div v-else-if="cfg" class="config-body">

        <!-- 远程仓库 -->
        <section class="cfg-section">
          <h4 class="section-title">{{ t('repoConfig.remotes') }}</h4>
          <div v-if="cfg.remotes.length === 0" class="empty-tip">
            {{ t('repoConfig.noRemotes') }}
          </div>
          <div v-for="rm in cfg.remotes" :key="rm.name" class="remote-card">
            <div class="remote-head">
              <span class="remote-name">{{ rm.name }}</span>
              <span class="remote-type-badge">{{
                rm.fetch_url.startsWith('http') ? 'HTTPS' :
                rm.fetch_url.startsWith('ssh') ? 'SSH' :
                rm.fetch_url.startsWith('git@') ? 'SSH' : t('repoConfig.localType')
              }}</span>
            </div>
            <div class="field-row">
              <span class="field-label">Fetch URL</span>
              <code class="field-value">{{ rm.fetch_url || '—' }}</code>
            </div>
            <div v-if="rm.push_url && rm.push_url !== rm.fetch_url" class="field-row">
              <span class="field-label">Push URL</span>
              <code class="field-value">{{ rm.push_url }}</code>
            </div>
          </div>
        </section>

        <!-- 基本信息 -->
        <section class="cfg-section">
          <h4 class="section-title">{{ t('repoConfig.basic') }}</h4>
          <div class="field-stack">
            <div class="field-row">
              <span class="field-label">user.name</span>
              <code class="field-value mono">{{ cfg.user_name || t('repoConfig.notConfigured') }}</code>
            </div>
            <div class="field-row">
              <span class="field-label">user.email</span>
              <code class="field-value mono">{{ cfg.user_email || t('repoConfig.notConfigured') }}</code>
            </div>
          </div>
        </section>

        <!-- 认证与 SSH -->
        <section class="cfg-section">
          <h4 class="section-title">{{ t('repoConfig.authSsh') }}</h4>

          <div class="field-row">
            <span class="field-label">credential.helper</span>
            <code class="field-value mono">{{ cfg.credential_helper || t('repoConfig.defaultHelper') }}</code>
          </div>

          <!-- SSH 私钥路径（id_rsa）可编辑 -->
          <div class="ssh-key-edit">
            <div class="field-label">{{ t('repoConfig.sshKeyPath') }}</div>
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

          <div v-if="cfg.core_ssh_command || cfg.git_ssh_command_env || cfg.ssh_auth_sock_env" class="env-box">
            <div v-if="cfg.core_ssh_command" class="field-row">
              <span class="field-label">core.sshCommand</span>
              <code class="field-value mono">{{ cfg.core_ssh_command }}</code>
            </div>
            <div v-if="cfg.git_ssh_command_env" class="field-row">
              <span class="field-label">GIT_SSH_COMMAND (env)</span>
              <code class="field-value mono">{{ cfg.git_ssh_command_env }}</code>
            </div>
            <div v-if="cfg.ssh_auth_sock_env" class="field-row">
              <span class="field-label">SSH_AUTH_SOCK (env)</span>
              <code class="field-value mono">{{ cfg.ssh_auth_sock_env }}</code>
            </div>
          </div>

          <div class="ssh-grid">
            <div v-for="pair in sshKeyPairs" :key="pair.private?.path" class="ssh-key-card">
              <div class="ssh-name">
                {{ pair.private ? pair.private.path.split(/[/\\]/).pop() : '' }}
              </div>
              <div class="ssh-status">
                <span
                  class="status-dot"
                  :class="pair.private?.exists ? 'ok' : 'missing'"
                />
                {{ pair.private?.exists ? t('repoConfig.exists') : t('repoConfig.missing') }}
              </div>
              <div v-if="pair.public" class="ssh-status subtle">
                {{ t('repoConfig.publicKey') }}
                <span
                  class="status-dot"
                  :class="pair.public.exists ? 'ok' : 'missing'"
                />
                {{ pair.public.exists ? t('repoConfig.exists') : t('repoConfig.missing') }}
              </div>
              <div class="ssh-path" :title="pair.private?.path">
                {{ pair.private?.path }}
              </div>
            </div>
          </div>
        </section>

        <!-- 原始 config（可折叠） -->
        <section class="cfg-section raw-section">
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
  max-width: 620px;
  max-height: 85vh;
  overflow-y: auto;
}

.repo-path-desc {
  font-family: Consolas, Monaco, monospace;
  font-size: 11.5px;
  word-break: break-all;
}

.loading,
.error-message {
  font-size: 12.5px;
  padding: 12px 0;
  text-align: center;
}
.error-message {
  color: var(--danger-color);
}

.config-body {
  display: flex;
  flex-direction: column;
  gap: 14px;
  padding-top: 4px;
}

.cfg-section {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.section-title {
  margin: 0;
  font-size: 12px;
  font-weight: 600;
  color: var(--text-tertiary);
  text-transform: uppercase;
  letter-spacing: 0.04em;
  padding-bottom: 4px;
  border-bottom: 1px solid var(--border-light);
}

.empty-tip {
  font-size: 12px;
  color: var(--text-tertiary);
  padding: 8px 0;
}

/* 远程 */
.remote-card {
  border: 1px solid var(--border-medium);
  border-radius: 8px;
  padding: 10px 12px;
  background: var(--bg-tertiary);
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.remote-head {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 2px;
}
.remote-name {
  font-weight: 600;
  font-size: 13px;
  color: var(--text-primary);
}
.remote-type-badge {
  font-size: 10.5px;
  padding: 1px 7px;
  border-radius: 10px;
  background: var(--brand-bg);
  color: var(--brand-primary);
  font-weight: 500;
}

.field-row {
  display: flex;
  align-items: baseline;
  gap: 8px;
  font-size: 12.5px;
}
.field-label {
  color: var(--text-tertiary);
  flex-shrink: 0;
  min-width: 110px;
}
.field-value {
  color: var(--text-primary);
  word-break: break-all;
}
.field-value.mono {
  font-family: Consolas, Monaco, monospace;
}

.field-stack {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

/* SSH 私钥路径编辑 */
.ssh-key-edit {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 10px 12px;
  border: 1px solid var(--border-medium);
  border-radius: 8px;
  background: var(--bg-tertiary);
}
.ssh-key-edit .field-label {
  min-width: 0;
}
.ssh-key-input-row {
  display: flex;
  gap: 6px;
  align-items: center;
}
.ssh-key-input {
  flex: 1;
  min-width: 0;
  padding: 5px 8px;
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
.ssh-key-hint {
  font-size: 11px;
  color: var(--text-tertiary);
  line-height: 1.5;
}
.ssh-key-msg {
  font-size: 11.5px;
  color: var(--success-color, #22c55e);
}
.ssh-key-msg.error {
  color: var(--danger-color, #ef4444);
}

/* SSH 私钥路径编辑 */
.ssh-key-edit {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 10px 12px;
  border: 1px solid var(--border-medium);
  border-radius: 8px;
  background: var(--bg-tertiary);
}
.ssh-key-edit .field-label {
  min-width: 0;
}
.ssh-key-input-row {
  display: flex;
  gap: 6px;
  align-items: center;
}
.ssh-key-input {
  flex: 1;
  min-width: 0;
  padding: 5px 8px;
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
.ssh-key-hint {
  font-size: 11px;
  color: var(--text-tertiary);
  line-height: 1.5;
}
.ssh-key-msg {
  font-size: 11.5px;
  color: var(--success-color, #22c55e);
}
.ssh-key-msg.error {
  color: var(--danger-color, #ef4444);
}

.env-box {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 8px 10px;
  border-radius: 6px;
  background: var(--bg-tertiary);
  border: 1px dashed var(--border-medium);
}

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
  background: var(--bg-tertiary);
}
.ssh-name {
  font-weight: 600;
  font-size: 12.5px;
  color: var(--text-primary);
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
  background: var(--success-color, #22c55e);
  box-shadow: 0 0 0 2px var(--success-bg, #22c55e22);
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
  background: var(--bg-tertiary);
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
