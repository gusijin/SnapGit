// GitHub OAuth Device Authorization Grant（设备授权流）
// 对应 SmartGit 的「跳转 GitHub 授权」体验：点按钮 → 自动打开 github.com/login/device 并填好验证码
// → 用户在网页点授权 → 客户端自动拿到 access_token 存好并重试推送。无需本地回调端口，macOS/Windows 通用。
//
// ⚠️ 必填：在 GitHub 注册一个 OAuth App（https://github.com/settings/developers → New OAuth App）
//   - Application name：任意（如 SnapGit）
//   - Homepage / Authorization callback URL：可随便填（Device Flow 不需要回调地址）
//   复制生成的 Client ID，通过以下任一方式提供给本模块（优先级从高到低）：
//   1) 在 RepoConfigDialog「认证与 SSH」标签页的设置界面填写（持久化到 localStorage）
//   2) 项目根目录 .env 写入：VITE_GITHUB_CLIENT_ID=你的client_id
//   3) 以上都没有则使用占位常量，使用前必须配置其中之一
import { open } from '@tauri-apps/plugin-shell'

// 运行时可配置：优先级 localStorage（设置界面填写）> .env (VITE_GITHUB_CLIENT_ID) > 占位常量。
// 这样用户无需改源码或 .env，直接在 RepoConfigDialog 的「认证与 SSH」标签页填写并持久化。
const STORAGE_KEY = 'snapgit.github_client_id'
const DEFAULT_CLIENT_ID = 'YOUR_GITHUB_OAUTH_CLIENT_ID'

function readStoredClientId(): string {
  try {
    return localStorage.getItem(STORAGE_KEY) || ''
  } catch {
    return ''
  }
}

let githubClientId: string =
  readStoredClientId() ||
  (import.meta.env.VITE_GITHUB_CLIENT_ID as string | undefined)?.trim() ||
  DEFAULT_CLIENT_ID

export function getGitHubClientId(): string {
  return githubClientId
}

/** 保存 GitHub OAuth App 的 Client ID（应用级、持久化到 localStorage） */
export function setGitHubClientId(id: string): void {
  const v = id.trim()
  githubClientId = v || DEFAULT_CLIENT_ID
  try {
    if (v) localStorage.setItem(STORAGE_KEY, v)
    else localStorage.removeItem(STORAGE_KEY)
  } catch {
    // 隐私模式等 localStorage 不可用时静默降级（仅本次会话生效）
  }
}

export const isGitHubClientIdConfigured = (): boolean =>
  githubClientId !== '' && githubClientId !== DEFAULT_CLIENT_ID

/**
 * 从远端 URL 里提取主机名，兼容各种写法：
 *   https://github.com/u/r.git | http://... | ssh://git@github.com/u/r.git | git@github.com:u/r.git
 * 提取失败返回空串。
 */
function extractHost(url: string): string {
  const s = (url || '').trim()
  if (!s) return ''
  // scp-like 语法必须优先处理，否则 host:path 会被 URL 解析成端口/协议
  const scp = s.match(/^[a-z0-9._~+-]+@([^:/]+):/i)
  if (scp) return scp[1].toLowerCase()
  const withScheme = /^[a-z][a-z0-9+.-]*:\/\//i.test(s) ? s : `https://${s}`
  try {
    return new URL(withScheme).hostname.toLowerCase()
  } catch {
    return ''
  }
}

/**
 * 判断远端是否为 GitHub。仅认 github.com 及其子域（gist.github.com 等）。
 * GitHub Enterprise 自建域名无法从 URL 推断，一律按非 GitHub 处理（不给设备授权入口）。
 */
export function isGitHubRemoteUrl(url: string): boolean {
  const host = extractHost(url)
  if (!host) return false
  return host === 'github.com' || host.endsWith('.github.com')
}

const DEVICE_CODE_URL = 'https://github.com/login/device/code'
const ACCESS_TOKEN_URL = 'https://github.com/login/oauth/access_token'

export interface GitHubDeviceCode {
  device_code: string
  user_code: string
  verification_uri: string
  expires_in: number
  interval: number
}

export type GitHubAuthStatus =
  | { type: 'waiting'; userCode: string; verificationUri: string }
  | { type: 'authorized'; token: string }
  | { type: 'denied' }
  | { type: 'expired' }
  | { type: 'notconfigured' }
  | { type: 'error'; message: string }

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

/**
 * 启动 GitHub 设备授权流。
 * @param onStatus 状态回调，用于驱动 UI（展示验证码 / 成功 / 失败）
 * @param signal   可传入 AbortSignal 在对话框关闭时中止轮询
 * @returns 成功返回 access_token，否则返回 null
 */
export async function startGitHubDeviceAuth(
  onStatus: (s: GitHubAuthStatus) => void,
  signal?: AbortSignal,
): Promise<string | null> {
  // 1. 请求设备码
  let device: GitHubDeviceCode
  try {
    const res = await fetch(DEVICE_CODE_URL, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', Accept: 'application/json' },
      body: JSON.stringify({ client_id: getGitHubClientId(), scope: 'repo' }),
      signal,
    })
    if (!res.ok) {
      onStatus({ type: 'error', message: `GitHub 设备码请求失败 (HTTP ${res.status})` })
      return null
    }
    device = (await res.json()) as GitHubDeviceCode
  } catch (e: any) {
    if (signal?.aborted) return null
    onStatus({ type: 'error', message: `请求设备码异常: ${String(e?.message || e)}` })
    return null
  }

  // 2. 通知 UI 展示验证码，并自动打开浏览器到验证页
  onStatus({
    type: 'waiting',
    userCode: device.user_code,
    verificationUri: device.verification_uri,
  })
  try {
    await open(device.verification_uri)
  } catch {
    // 自动打开失败不致命，用户可手动复制链接打开
  }

  // 3. 按 interval 轮询 access token，直到授权成功或超时/被拒
  let intervalMs = Math.max((device.interval || 5) * 1000, 1000)
  const deadline = Date.now() + device.expires_in * 1000

  while (Date.now() < deadline) {
    if (signal?.aborted) return null
    await sleep(intervalMs)
    if (signal?.aborted) return null

    try {
      const r = await fetch(ACCESS_TOKEN_URL, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Accept: 'application/json' },
        body: JSON.stringify({
          client_id: getGitHubClientId(),
          device_code: device.device_code,
          grant_type: 'urn:ietf:params:oauth:grant-type:device_code',
        }),
        signal,
      })
      const json = (await r.json()) as Record<string, string>

      if (json.access_token) {
        onStatus({ type: 'authorized', token: json.access_token })
        return json.access_token
      }

      switch (json.error) {
        case 'authorization_pending':
          continue
        case 'slow_down':
          // 服务端要求拉长轮询间隔（+5s）
          intervalMs += 5000
          continue
        case 'expired_token':
          onStatus({ type: 'expired' })
          return null
        case 'access_denied':
          onStatus({ type: 'denied' })
          return null
        default:
          onStatus({
            type: 'error',
            message: json.error_description || json.error || '未知错误',
          })
          return null
      }
    } catch (e: any) {
      if (signal?.aborted) return null
      onStatus({ type: 'error', message: `轮询授权结果异常: ${String(e?.message || e)}` })
      return null
    }
  }

  onStatus({ type: 'expired' })
  return null
}
