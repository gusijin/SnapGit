import { createI18n } from 'vue-i18n'
import { LocaleRegistry, fallbackLocale, getLocaleMessages } from './locales'

export const STORAGE_KEY = 'snapgit-locale'

/**
 * 读取已保存的语言偏好；未保存或非法时回退到浏览器语言，
 * 都不匹配则使用 fallbackLocale。
 */
export function resolveLocale(): string {
  try {
    const saved = localStorage.getItem(STORAGE_KEY)
    if (saved && LocaleRegistry.some((l) => l.code === saved)) {
      return saved
    }
  } catch (e) {
    console.error('[SnapGit] 读取语言偏好失败:', e)
  }

  const navLang = (navigator.language || '').toLowerCase()
  const matched = LocaleRegistry.find((l) => l.code.toLowerCase() === navLang)
  return matched ? matched.code : fallbackLocale
}

export function saveLocale(code: string) {
  try {
    localStorage.setItem(STORAGE_KEY, code)
  } catch (e) {
    console.error('[SnapGit] 保存语言偏好失败:', e)
  }
}

// 共享单例：main.ts 与 editor-main.ts 引入的是同一个 i18n 实例，
// 切换语言后两窗口保持一致。
export const i18n = createI18n({
  legacy: false, // 组合式 API 模式
  globalInjection: true, // 模板中可直接使用 $t
  locale: resolveLocale(),
  fallbackLocale,
  missingWarn: false,
  fallbackWarn: false,
  // 消息对象结构庞大且各处深度嵌套，vue-i18n 的深度类型推导在大消息集下易触发
  // "excessively deep" 或重载不可赋值错误。此处作为配置边界统一用 any 放宽，
  // 不影响业务代码通过 useI18n/$t 获得按 key 的类型提示。
  messages: Object.fromEntries(LocaleRegistry.map((l) => [l.code, l.messages])) as any,
})

/**
 * 切换应用语言并持久化。code 必须是 LocaleRegistry 中已登记的语言。
 */
export function setLocale(code: string) {
  ;(i18n.global as unknown as { locale: { value: string } }).locale.value = code
  saveLocale(code)
}

// 保持 type 提示，未来若引入额外类型可扩展至此
export type { LocaleMeta } from './locales'

// 导出便于在非组件文件中使用（如 console 无需翻译，但业务逻辑可能需要）
export const getMessage = (key: string): string =>
  (i18n.global as unknown as { t: (k: string) => string }).t(key)

// 便捷暴露：新增语言入口供 UI 渲染语言选择列表
export const availableLocales = (): { code: string; label: string }[] =>
  LocaleRegistry.map((l) => ({ code: l.code, label: l.label }))

/** getLocaleMessages 的类型化包装，避免重复析构显示名 */
export { getLocaleMessages }