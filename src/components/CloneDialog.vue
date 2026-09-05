<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { cloneRepository, openFolderDialog } from '../api/git'
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

const emit = defineEmits(['close', 'cloned'])
const { t } = useI18n()

/** 0 = Repository, 1 = Selection, 2 = Local Directory */
const step = ref(0)

// Step 1：远程仓库地址
const sourceUrl = ref('')

// Step 2：克隆选项
const includeSubmodules = ref(true)
const checkoutBranch = ref('') // 空 = 远程默认分支
const fetchTags = ref(true)

// Step 3：最终目标目录（用户选的就是最终路径）
const targetDir = ref('')

// 初始目标目录：默认放用户主目录下（用户可以自己改）
onMounted(async () => {
  try {
    const { homeDir } = await import('@tauri-apps/api/path')
    targetDir.value = await homeDir()
  } catch {
    // 降级：什么都不填
  }
})

const isCloning = ref(false)
const error = ref('')
const progress = ref('')

function guardClose(e: Event) {
  if (isCloning.value) e.preventDefault()
}
function onOpenChange(open: boolean) {
  if (!open) emit('close')
}

function canNext(): boolean {
  if (step.value === 0) return !!sourceUrl.value.trim()
  if (step.value === 1) return true
  return !!targetDir.value.trim()
}
function canPrev(): boolean {
  return step.value > 0 && !isCloning.value
}
function canFinish(): boolean {
  return step.value === 2 && !!targetDir.value.trim() && !isCloning.value
}

function nextStep() {
  if (!canNext()) return
  if (step.value < 2) {
    error.value = ''
    step.value++
  }
}
function prevStep() {
  if (!canPrev()) return
  error.value = ''
  step.value--
}

async function pickTargetDir() {
  try {
    const p = await openFolderDialog()
    if (p) targetDir.value = p
  } catch (e) {
    error.value = String(e)
  }
}

async function handleClone() {
  if (!canFinish()) return
  error.value = ''
  isCloning.value = true
  progress.value = t('clone.progress')
  try {
    const resultPath = await cloneRepository({
      url: sourceUrl.value.trim(),
      targetDir: targetDir.value.trim(),
      branch: checkoutBranch.value.trim(),
      includeSubmodules: includeSubmodules.value,
      fetchTags: fetchTags.value,
    })
    progress.value = t('clone.done')
    emit('cloned', resultPath)
    emit('close')
  } catch (e) {
    error.value = String(e)
    progress.value = ''
  } finally {
    isCloning.value = false
  }
}
</script>

<template>
  <Dialog :open="true" @update:open="onOpenChange">
    <DialogContent
      class="clone-dialog"
      @pointer-down-outside="guardClose"
      @escape-keydown="guardClose"
    >
      <DialogHeader>
        <DialogTitle>{{ t('clone.title') }}</DialogTitle>
        <DialogDescription>
          {{ t('clone.desc') }}
        </DialogDescription>
      </DialogHeader>

      <div class="clone-body">
        <!-- 左侧步骤导航 -->
        <aside class="step-nav">
          <div
            v-for="(labelKey, idx) in ['clone.stepRepo', 'clone.stepOptions', 'clone.stepLocalDir']"
            :key="idx"
            class="step-item"
            :class="{
              active: step === idx,
              done: step > idx,
            }"
          >
            <span class="step-num">{{ idx + 1 }}</span>
            <span class="step-label">{{ t(labelKey) }}</span>
          </div>
        </aside>

        <!-- 右侧内容 -->
        <section class="step-content">
          <!-- Step 0：Repository -->
          <template v-if="step === 0">
            <h3 class="step-title">{{ t('clone.stepRepoTitle') }}</h3>
            <p class="step-desc">{{ t('clone.stepRepoDesc') }}</p>

            <div class="field">
              <Label for="clone-url">{{ t('clone.repoUrl') }}</Label>
              <Input
                id="clone-url"
                v-model="sourceUrl"
                :placeholder="t('clone.repoUrlPlaceholder')"
                autocomplete="off"
                spellcheck="false"
              />
              <p class="hint">{{ t('clone.protocolHint') }}</p>
            </div>
          </template>

          <!-- Step 1：Selection -->
          <template v-else-if="step === 1">
            <h3 class="step-title">{{ t('clone.stepOptionsTitle') }}</h3>
            <p class="step-desc">{{ t('clone.stepOptionsDesc') }}</p>

            <div class="check-row">
              <input id="opt-submodule" type="checkbox" v-model="includeSubmodules" />
              <label for="opt-submodule">{{ t('clone.includeSubmodules') }}</label>
            </div>

            <div class="field">
              <Label for="opt-branch">{{ t('clone.checkoutBranchField') }}</Label>
              <Select v-model="checkoutBranch">
                <SelectTrigger id="opt-branch">
                  <SelectValue :placeholder="t('clone.defaultBranchPlaceholder')" />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="">{{ t('clone.defaultBranch') }}</SelectItem>
                  <SelectItem value="main">main</SelectItem>
                  <SelectItem value="master">master</SelectItem>
                  <SelectItem value="develop">develop</SelectItem>
                </SelectContent>
              </Select>
            </div>

            <div class="check-row">
              <input id="opt-tags" type="checkbox" v-model="fetchTags" />
              <label for="opt-tags">{{ t('clone.fetchTags') }}</label>
            </div>
          </template>

          <!-- Step 2：Local Directory -->
          <template v-else>
            <h3 class="step-title">{{ t('clone.stepLocalTitle') }}</h3>
            <p class="step-desc">{{ t('clone.stepLocalDesc') }}</p>

            <div class="field">
              <Label for="clone-target">{{ t('clone.targetDir') }}</Label>
              <div class="path-row">
                <Input
                  id="clone-target"
                  v-model="targetDir"
                  :placeholder="t('clone.targetPlaceholder')"
                />
                <Button variant="outline" @click="pickTargetDir" type="button">
                  {{ t('clone.browse') }}
                </Button>
              </div>
              <p class="hint">
                {{ t('clone.dirHint') }}
              </p>
            </div>
          </template>

          <!-- 错误 / 进度 -->
          <div v-if="error" class="error-message">{{ error }}</div>
          <div v-else-if="progress" class="progress-message">{{ progress }}</div>
        </section>
      </div>

      <DialogFooter>
        <Button variant="outline" @click="emit('close')" :disabled="isCloning">
          {{ t('clone.cancel') }}
        </Button>
        <Button variant="outline" @click="prevStep" :disabled="!canPrev()">
          {{ t('clone.prev') }}
        </Button>
        <template v-if="step < 2">
          <Button @click="nextStep" :disabled="!canNext()">{{ t('clone.next') }}</Button>
        </template>
        <template v-else>
          <Button @click="handleClone" :disabled="!canFinish()">
            {{ isCloning ? t('clone.cloning') : t('clone.finish') }}
          </Button>
        </template>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>

<style scoped>
.clone-dialog {
  max-width: 640px;
  padding: 0;
}

.clone-body {
  display: flex;
  gap: 20px;
  min-height: 300px;
}

.step-nav {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 110px;
  padding-top: 6px;
  border-right: 1px solid var(--border-medium);
  padding-right: 16px;
}

.step-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 8px;
  border-radius: 6px;
  color: var(--text-tertiary);
  font-size: 12.5px;
  font-weight: 500;
  transition: all 0.15s ease;
}

.step-num {
  width: 20px;
  height: 20px;
  border-radius: 50%;
  background: var(--bg-tertiary);
  border: 1px solid var(--border-medium);
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 11px;
  color: var(--text-tertiary);
  flex-shrink: 0;
}

.step-item.active {
  background: var(--brand-bg);
  color: var(--brand-primary);
}
.step-item.active .step-num {
  background: var(--brand-primary);
  border-color: var(--brand-primary);
  color: var(--brand-foreground, #fff);
}

.step-item.done {
  color: var(--text-secondary);
}
.step-item.done .step-num {
  background: var(--success-bg, #22c55e22);
  border-color: var(--success-color, #22c55e);
  color: var(--success-color, #22c55e);
}

.step-content {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 14px;
  padding-bottom: 4px;
}

.step-title {
  font-size: 15px;
  font-weight: 600;
  color: var(--text-primary);
  margin: 0;
}

.step-desc {
  font-size: 12px;
  color: var(--text-tertiary);
  margin: -6px 0 2px;
}

.field {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.hint {
  font-size: 11px;
  color: var(--text-tertiary);
  margin: 0;
  line-height: 1.5;
}

.path-row {
  display: flex;
  gap: 6px;
}
.path-row Input {
  flex: 1;
}

.check-row {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  color: var(--text-secondary);
  cursor: pointer;
}
.check-row input {
  accent-color: var(--brand-primary);
  width: 15px;
  height: 15px;
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

.progress-message {
  font-size: 12px;
  color: var(--brand-primary);
  padding: 4px 2px;
}
</style>
