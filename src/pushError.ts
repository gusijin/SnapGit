/**
 * Git 操作失败信息的分类与清洗。
 *
 * 背景：`git push/pull` 失败时，stderr 里除了真正的失败原因，还会混进大量与本次
 * 操作无关的噪声：OpenSSH 后量子密钥交换警告、known_hosts 自动写入提示，以及
 * 代码托管平台回传的 `remote:` 空行 / 分隔线 / gRPC 传输层错误。直接把这些原文
 * 铺到「操作失败」弹窗里既吓人又没有信息量，真正的 `fatal:` 原因还被埋在最后。
 *
 * 这里统一做两件事：
 *   1) 清洗掉这类噪声行，只留下与操作本身相关的部分（拉取/推送共用 cleanGitError）；
 *   2) 归类推送失败原因，其中「非快进（远程有新提交）」这一档供 UI 给出「拉取并推送」出口，
 *      对齐 IDEA / SmartGit 的推送被拒处理方式。
 */

/** 与本次操作无关、需要剔除的 stderr 噪声行（推送、拉取共用）。 */
const NOISE_LINE_PATTERNS: RegExp[] = [
  // OpenSSH 后量子密钥交换警告（3 行）
  /^\s*\*\*\s/,
  /^\s*See\s+https?:\/\/openssh\.com\/pq\.html\s*$/i,
  // known_hosts 自动写入提示
  /^\s*Warning:\s+Permanently added .*to the list of known hosts\.?\s*$/i,
  // 远端回显的空白行 / 纯分隔线（GitCode / GitLab banner，无任何信息量）
  /^\s*remote:\s*$/,
  /^\s*remote:\s*-{3,}\s*$/,
  // 代码托管平台 gRPC 传输层错误：连接不上后端，不是仓库/凭证问题，对用户无操作性指引
  /^\s*remote:\s*rpc error:.*TransientFailure/i,
  /^\s*connection error:\s*desc\s*=\s*"transport: Error while dialing/i,
]

/** 剔除噪声行、压缩连续空行并去掉首尾空行，得到可读的裸错误文本。 */
export function cleanGitError(raw: string): string {
  if (!raw) return ''
  const kept = raw.split(/\r?\n/).filter((ln) => !NOISE_LINE_PATTERNS.some((re) => re.test(ln)))
  // 噪声行被剔除后常留下一串空行；压成一个空行保留段落结构（认证失败文案里有 \n\n 分段）
  const compact: string[] = []
  for (const ln of kept) {
    if (ln.trim() === '' && compact[compact.length - 1]?.trim() === '') continue
    compact.push(ln)
  }
  while (compact.length > 0 && compact[0].trim() === '') compact.shift()
  while (compact.length > 0 && compact[compact.length - 1].trim() === '') compact.pop()
  return compact.join('\n')
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
