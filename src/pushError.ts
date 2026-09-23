/**
 * 推送失败信息的分类与清洗。
 *
 * 背景：`git push` 因「非快进」被拒时，stderr 里除了真正的拒绝原因，还会混进一段
 * 与本次操作无关的 OpenSSH 提示（后量子密钥交换警告、known_hosts 自动写入提示等）。
 * 直接把这些原文铺到弹窗/toast 里既吓人又没有信息量，也没有给出可操作的下一步。
 *
 * 这里统一做两件事：
 *   1) 清洗掉这类噪声行，只留下与推送本身相关的部分；
 *   2) 归类失败原因，其中「非快进（远程有新提交）」这一档供 UI 给出「拉取并推送」出口，
 *      对齐 IDEA / SmartGit 的推送被拒处理方式。
 */

/** 与本次推送无关、需要剔除的 stderr 噪声行。 */
const NOISE_LINE_PATTERNS: RegExp[] = [
  // OpenSSH 后量子密钥交换警告（3 行）
  /^\s*\*\*\s/,
  /^\s*See\s+https?:\/\/openssh\.com\/pq\.html\s*$/i,
  // known_hosts 自动写入提示
  /^\s*Warning:\s+Permanently added .*to the list of known hosts\.?\s*$/i,
]

/** 剔除噪声行并去掉首尾空行，得到可读的裸错误文本。 */
export function cleanGitError(raw: string): string {
  if (!raw) return ''
  const kept = raw.split(/\r?\n/).filter((ln) => !NOISE_LINE_PATTERNS.some((re) => re.test(ln)))
  while (kept.length > 0 && kept[0].trim() === '') kept.shift()
  while (kept.length > 0 && kept[kept.length - 1].trim() === '') kept.pop()
  return kept.join('\n')
}

/**
 * 是否属于「推送被拒：远程分支领先（非快进）」。
 * git 的英文输出包含 `! [rejected]`、`non-fast-forward`、`failed to push some refs`、
 * `Updates were rejected because the tip of your current branch is behind` 等；
 * 这里做宽松匹配以兼容不同 git 版本的措辞。
 */
export function isNonFastForward(raw: string): boolean {
  const s = (raw || '').toLowerCase()
  if (s.includes('non-fast-forward') || s.includes('failed to push some refs')) return true
  if (s.includes('updates were rejected') || s.includes('fetch first')) return true
  return s.includes('[rejected]') && s.includes('behind')
}

export type PushErrorKind = 'non-ff' | 'credential' | 'ssh' | 'other'

export interface PushErrorInfo {
  kind: PushErrorKind
  /** 清洗后的可读错误文本（已剔除噪声行） */
  detail: string
}

/**
 * 归类推送失败原因。
 * - non-ff     ：远程有本地没有的新提交（非快进被拒）→ 引导「拉取并推送」
 * - credential ：HTTPS 凭证失败 → 引导填令牌 / credential helper
 * - ssh        ：SSH 公钥未通过 → 引导检查 SSH 配置（不再强求令牌）
 * - other      ：其它 → 直接展示清洗后的原文
 */
export function classifyPushError(raw: string): PushErrorInfo {
  const detail = cleanGitError(raw)
  const lower = detail.toLowerCase()
  if (isNonFastForward(detail)) return { kind: 'non-ff', detail }
  if (
    lower.includes('permission denied (publickey)') ||
    lower.includes('could not read from remote repository') ||
    lower.includes('ssh 公钥认证未通过')
  ) {
    return { kind: 'ssh', detail }
  }
  if (
    lower.includes('authentication failed') ||
    lower.includes('access denied') ||
    lower.includes('could not read username') ||
    lower.includes('terminal prompts disabled') ||
    lower.includes('认证失败')
  ) {
    return { kind: 'credential', detail }
  }
  return { kind: 'other', detail }
}
