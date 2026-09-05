import { ref, watch } from 'vue'

export type ThemeName = 'dark' | 'light'

const STORAGE_KEY = 'snapgit-theme'

const theme = ref<ThemeName>(loadTheme())

function loadTheme(): ThemeName {
  const saved = localStorage.getItem(STORAGE_KEY)
  if (saved === 'dark' || saved === 'light') {
    return saved
  }
  // First time: default to light
  return 'light'
}

function applyTheme(t: ThemeName) {
  document.documentElement.setAttribute('data-theme', t)
}

export function useTheme() {
  watch(theme, (t) => {
    applyTheme(t)
    localStorage.setItem(STORAGE_KEY, t)
  }, { immediate: true })

  function toggleTheme() {
    theme.value = theme.value === 'dark' ? 'light' : 'dark'
  }

  function setTheme(t: ThemeName) {
    theme.value = t
  }

  return { theme, toggleTheme, setTheme }
}
