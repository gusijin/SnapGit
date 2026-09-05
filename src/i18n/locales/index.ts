import zhCNMessages from './zh-CN'
import enUSMessages from './en-US'

/**
 * 语言注册表：集中登记所有支持的语言。
 * 新增语言时只需：
 *  1. 新建 locales/<code>.ts 并导出默认消息对象
 *  2. 在本文件引入并加入 LocaleRegistry
 * 无需修改业务代码。业务代码通过 useI18n / $t 使用 key。
 */
export interface LocaleMeta {
  code: string
  label: string // 语言显示名（使用该语言自身的写法）
  messages: Record<string, unknown>
}

export const LocaleRegistry: LocaleMeta[] = [
  { code: 'zh-CN', label: '简体中文', messages: zhCNMessages },
  { code: 'en-US', label: 'English', messages: enUSMessages },
]

export const fallbackLocale = 'en-US'

export function getLocaleMessages(code: string): Record<string, unknown> {
  return LocaleRegistry.find((l) => l.code === code)?.messages || enUSMessages
}