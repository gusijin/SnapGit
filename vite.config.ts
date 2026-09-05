import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import tailwindcss from '@tailwindcss/vite'
import { fileURLToPath, URL } from 'node:url'

// 入口文件无法被 HMR 边界接受，改动它们时 Vite 会广播 full-reload，
// 导致 SnapGit 两个 webview 整页白屏重载（用 SnapGit 打开自身源码仓库、
// 外部保存 src/main.ts 等文件时的「整窗闪白」）。
// 应用面板数据由自身 fs 监听刷新，不依赖页面重载，因此对入口文件直接
// 吞掉热更新广播；其余组件/样式仍走正常 HMR。
// 注意：被吞掉的入口级改动需要手动刷新窗口（Ctrl+R / 重启 dev）才会生效。
const SUPPRESSED_ENTRY_FILES = ['src/main.ts', 'src/editor-main.ts']

const suppressEntryFullReload = () => ({
  name: 'snapgit-suppress-entry-full-reload',
  apply: 'serve' as const,
  handleHotUpdate(ctx: { file: string; server: any }) {
    const normalized = ctx.file.replace(/\\/g, '/')
    if (SUPPRESSED_ENTRY_FILES.some((f) => normalized.endsWith(f))) {
      ctx.server.config.logger.info(
        `[snapgit] 已跳过入口文件更新广播（不触发整页刷新）: ${normalized}`,
      )
      return []
    }
    return undefined
  },
})

export default defineConfig({
  plugins: [vue(), tailwindcss(), suppressEntryFullReload()],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url))
    }
  },
  // Dev 模式多窗口加速：默认 vite 只扫描主窗口(index.html)依赖并预打包，
  // 编辑窗口(editor.html)首次打开时才触发依赖扫描+预打包，会卡 1~3 秒。
  // 这里显式把两个入口都纳入扫描、并把重依赖列出来，dev server 启动时即全部预打包，
  // 任何窗口第一次打开都直接拉预打包好的单文件，不再现场扫描。
  optimizeDeps: {
    entries: ['index.html', 'editor.html'],
    include: [
      'vue',
      'vue-router',
      'vue-i18n',
      '@vueuse/core',
      '@tauri-apps/api',
      '@tauri-apps/api/window',
      'lucide-vue-next',
      'reka-ui',
      'class-variance-authority',
      'clsx',
      'tailwind-merge',
    ],
  },
  build: {
    rollupOptions: {
      input: {
        main: fileURLToPath(new URL('./index.html', import.meta.url)),
        editor: fileURLToPath(new URL('./editor.html', import.meta.url)),
      },
    },
  },
})
