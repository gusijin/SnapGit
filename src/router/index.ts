import { createRouter, createWebHashHistory } from 'vue-router'

const router = createRouter({
  // hash 模式：生产环境（tauri://localhost）与独立编辑窗口（index.html#/editor）均无需服务器路由支持
  history: createWebHashHistory(),
  routes: [
    {
      path: '/',
      name: 'Repository',
      component: () => import('../views/Repository.vue')
    },
    {
      path: '/editor',
      name: 'Editor',
      component: () => import('../views/EditorWindow.vue')
    }
  ]
})

export default router
