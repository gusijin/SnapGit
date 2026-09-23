/**
 * 词级（token 级）差异工具 —— 供冲突解决页「行内差异」高亮使用，参考 IDEA / SmartGit 的合并编辑器。
 *
 * 设计目标：
 * - 纯前端、零依赖：不引入 diff 库，也不改动 Rust 端（diff 在 Rust 侧算，但这里只需两侧文本）。
 * - 与既有语法高亮「求交」：既能保留关键字配色，又能标出「变化的词」，
 *   形成「行级底色 + 词级高亮」两层差异表达（IDEA 合并编辑器的经典呈现）。
 * - 仅做 token 级 LCS，且只在「同一冲突块内按行对齐」的短行上运行，开销可控。
 */

import { tokenizeLine, type TokenType, type LangId } from './syntaxHighlight'

/** 词级差异分段：changed=true 表示该 token 只属于本侧（另一侧不存在） */
export interface WordChunk {
  text: string
  changed: boolean
}

/** 最终用于渲染的分段：token 语法类型 + 是否词级变化 */
export interface Segment {
  text: string
  type: TokenType
  changed: boolean
}

/**
 * 把一行拆成 token：连续单词字符 / 连续空白 / 单个其它字符。
 * 三者拼接严格等于原文（保证 char 级掩码与原文逐位对齐）。
 */
function tokenize(text: string): string[] {
  const out: string[] = []
  const re = /[A-Za-z0-9_]+|\s+|[^A-Za-z0-9_\s]/g
  let m: RegExpExecArray | null
  while ((m = re.exec(text)) !== null) out.push(m[0])
  return out
}

/**
 * 两侧文本的 token 级差异（LCS + 回溯）。
 * 返回各自分段序列，changed=true 的 token 即「只在这一侧出现」的词。
 */
export function diffWords(a: string, b: string): { left: WordChunk[]; right: WordChunk[] } {
  const A = tokenize(a)
  const B = tokenize(b)
  const n = A.length
  const m = B.length

  // dp[i][j] = LCS(A[i:], B[j:]) 的长度
  const dp: number[][] = Array.from({ length: n + 1 }, () => new Array<number>(m + 1).fill(0))
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      dp[i][j] = A[i] === B[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1])
    }
  }

  const left: WordChunk[] = []
  const right: WordChunk[] = []
  let i = 0
  let j = 0
  while (i < n && j < m) {
    if (A[i] === B[j]) {
      left.push({ text: A[i], changed: false })
      right.push({ text: B[j], changed: false })
      i++
      j++
    } else if (dp[i + 1][j] >= dp[i][j + 1]) {
      left.push({ text: A[i], changed: true })
      i++
    } else {
      right.push({ text: B[j], changed: true })
      j++
    }
  }
  while (i < n) {
    left.push({ text: A[i], changed: true })
    i++
  }
  while (j < m) {
    right.push({ text: B[j], changed: true })
    j++
  }
  return { left, right }
}

/** 纯语法分段（无词级变化标记），用于非冲突行 */
export function plainSegments(line: string, lang: LangId): Segment[] {
  return tokenizeLine(line, lang).map((t) => ({ text: t.text, type: t.type, changed: false }))
}

/** 整行都视为变化（一侧缺失的对齐行，如纯增/纯删） */
export function changedSegments(line: string, lang: LangId): Segment[] {
  return tokenizeLine(line, lang).map((t) => ({ text: t.text, type: t.type, changed: true }))
}

/**
 * 把「词级 changed 掩码」与「语法高亮 token」求交：
 * 同一语法 token 内按 changed 连续段切开，得到最小粒度的放大分段。
 */
export function mergeSegments(chunks: WordChunk[], line: string, lang: LangId): Segment[] {
  const mask = new Array<boolean>(line.length).fill(false)
  let p = 0
  for (const c of chunks) {
    if (c.changed) {
      for (let k = 0; k < c.text.length; k++) mask[p + k] = true
    }
    p += c.text.length
  }

  const segs: Segment[] = []
  let off = 0
  for (const t of tokenizeLine(line, lang)) {
    let s = 0
    for (let k = 1; k <= t.text.length; k++) {
      if (k === t.text.length || mask[off + k] !== mask[off + s]) {
        segs.push({ text: t.text.slice(s, k), type: t.type, changed: mask[off + s] })
        s = k
      }
    }
    off += t.text.length
  }
  return segs
}
