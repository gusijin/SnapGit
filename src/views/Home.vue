<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { openRepository, openFolderDialog, loadRecentRepositories, removeRecentRepository, saveRecentRepository } from '../api/git'
import type { RepositoryInfo } from '../types'

const { t } = useI18n()
const router = useRouter()
const isLoading = ref(false)
const recentRepos = ref<RepositoryInfo[]>([])

async function loadRecent() {
  try {
    recentRepos.value = await loadRecentRepositories()
  } catch (error) {
    console.error('Failed to load recent repositories:', error)
  }
}

async function handleOpenRepository() {
  isLoading.value = true
  try {
    const path = await openFolderDialog()
    
    if (path) {
      const repoInfo = await openRepository(path)
      await saveRecentRepository(repoInfo)
      router.push(`/repository/${encodeURIComponent(path)}`)
    }
  } catch (error) {
    console.error('Failed to open repository:', error)
  } finally {
    isLoading.value = false
  }
}

async function handleOpenRecent(repo: RepositoryInfo) {
  isLoading.value = true
  try {
    const repoInfo = await openRepository(repo.path)
    await saveRecentRepository(repoInfo)
    router.push(`/repository/${encodeURIComponent(repo.path)}`)
  } catch (error) {
    console.error('Failed to open repository:', error)
  } finally {
    isLoading.value = false
  }
}

async function handleRemoveRecent(path: string) {
  try {
    await removeRecentRepository(path)
    recentRepos.value = recentRepos.value.filter(r => r.path !== path)
  } catch (error) {
    console.error('Failed to remove recent repository:', error)
  }
}

onMounted(() => {
  loadRecent()
})
</script>

<template>
  <div class="home-container">
    <div class="hero-section">
      <div class="logo">
        <svg width="64" height="64" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <circle cx="12" cy="12" r="10"></circle>
          <path d="M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20"></path>
          <path d="M12 2a14.5 14.5 0 0 1 0 20 14.5 14.5 0 0 1 0-20"></path>
          <path d="M2 12h20"></path>
        </svg>
      </div>
      <h1>SnapGit</h1>
      <p class="subtitle">{{ t('app.subtitle') }}</p>
      
      <button 
        class="open-btn" 
        :disabled="isLoading"
        @click="handleOpenRepository"
      >
        <svg v-if="!isLoading" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"></path>
          <polyline points="17 8 12 3 7 8"></polyline>
          <line x1="12" y1="3" x2="12" y2="15"></line>
        </svg>
        <span v-if="isLoading">{{ t('home.loading') }}</span>
        <span v-else>{{ t('home.openRepo') }}</span>
      </button>
      
      <div v-if="recentRepos.length > 0" class="recent-section">
        <h2 class="recent-title">{{ t('recent.title') }}</h2>
        <div class="recent-list">
          <div 
            v-for="repo in recentRepos" 
            :key="repo.path" 
            class="recent-item"
            @click="handleOpenRecent(repo)"
          >
            <div class="recent-icon">
              <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M10 20l4-16"></path>
                <path d="M20 20l-4-4"></path>
                <path d="M4 20l4-4"></path>
              </svg>
            </div>
            <div class="recent-info">
              <span class="recent-name">{{ repo.name }}</span>
              <span class="recent-path">{{ repo.path }}</span>
            </div>
            <div class="recent-branch">{{ repo.current_branch }}</div>
            <button 
              class="recent-remove"
              @click.stop="handleRemoveRecent(repo.path)"
            >
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <line x1="18" y1="6" x2="6" y2="18"></line>
                <line x1="6" y1="6" x2="18" y2="18"></line>
              </svg>
            </button>
          </div>
        </div>
      </div>
      
      <div class="features">
        <div class="feature-item">
          <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"></path>
            <polyline points="14 2 14 8 20 8"></polyline>
            <line x1="16" y1="13" x2="8" y2="13"></line>
            <line x1="16" y1="17" x2="8" y2="17"></line>
            <polyline points="10 9 9 9 8 9"></polyline>
          </svg>
          <span>{{ t('features.code') }}</span>
        </div>
        <div class="feature-item">
          <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"></path>
            <polyline points="7 10 12 15 17 10"></polyline>
            <line x1="12" y1="15" x2="12" y2="3"></line>
          </svg>
          <span>{{ t('features.branch') }}</span>
        </div>
        <div class="feature-item">
          <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <circle cx="12" cy="12" r="10"></circle>
            <polyline points="12 6 12 12 16 14"></polyline>
          </svg>
          <span>{{ t('features.visualize') }}</span>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.home-container {
  height: 100%;
  display: flex;
  justify-content: center;
  align-items: center;
  background: linear-gradient(135deg, var(--bg-tertiary) 0%, var(--bg-primary) 100%);
  padding: 40px;
}

.hero-section {
  text-align: center;
  max-width: 600px;
  width: 100%;
}

.logo {
  color: var(--brand-primary);
  margin-bottom: 20px;
}

h1 {
  font-size: 48px;
  font-weight: 300;
  color: var(--text-bright);
  margin-bottom: 10px;
}

.subtitle {
  font-size: 18px;
  color: var(--text-tertiary);
  margin-bottom: 40px;
}

.open-btn {
  display: inline-flex;
  align-items: center;
  gap: 10px;
  padding: 14px 32px;
  font-size: 16px;
  font-weight: 500;
  color: #ffffff;
  background-color: var(--brand-primary);
  border: none;
  border-radius: 8px;
  cursor: pointer;
  transition: background-color 0.3s, transform 0.2s;
}

.open-btn:hover:not(:disabled) {
  background-color: var(--brand-hover);
  transform: translateY(-2px);
}

.open-btn:disabled {
  background-color: var(--brand-disabled);
  cursor: not-allowed;
}

.recent-section {
  margin-top: 40px;
  text-align: left;
}

.recent-title {
  font-size: 16px;
  font-weight: 600;
  color: var(--text-tertiary);
  margin-bottom: 12px;
}

.recent-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
  background-color: var(--bg-secondary);
  border-radius: 8px;
  padding: 8px;
}

.recent-item {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px;
  border-radius: 6px;
  cursor: pointer;
  transition: background-color 0.2s;
}

.recent-item:hover {
  background-color: var(--bg-hover);
}

.recent-icon {
  color: var(--brand-primary);
  flex-shrink: 0;
}

.recent-info {
  flex: 1;
  text-align: left;
  min-width: 0;
}

.recent-name {
  display: block;
  font-size: 14px;
  font-weight: 500;
  color: var(--text-bright);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.recent-path {
  display: block;
  font-size: 12px;
  color: var(--text-tertiary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.recent-branch {
  font-size: 12px;
  color: var(--brand-primary);
  background-color: var(--brand-bg);
  padding: 2px 8px;
  border-radius: 4px;
  flex-shrink: 0;
}

.recent-remove {
  background: none;
  border: none;
  color: var(--text-muted);
  cursor: pointer;
  padding: 4px;
  border-radius: 4px;
  transition: all 0.2s;
  flex-shrink: 0;
}

.recent-remove:hover {
  color: var(--danger-color);
  background-color: var(--danger-bg);
}

.features {
  display: flex;
  justify-content: center;
  gap: 40px;
  margin-top: 60px;
}

.feature-item {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 10px;
  color: var(--text-tertiary);
}

.feature-item svg {
  color: var(--brand-primary);
}
</style>
