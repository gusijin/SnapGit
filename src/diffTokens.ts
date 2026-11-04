import type { DiffSegment } from './types'
import type { Token } from './syntaxHighlight'

/** 语法 token 叠加「词级差异」标记后的渲染单元 */
export interface StyledToken {
  text: string
  type: string
  changed: boolean
}

/**
 * 把后端 diff_chars 给出的分段（changed=true 即真正改动的那几段）换算成字符区间。
 * 相邻的 changed 段合并，少渲染一层 span。
 */
export function changedRanges(segs: DiffSegment[] | undefined): Array<[number, number]> {
  const ranges: Array<[number, number]> = []
  if (!segs || segs.length === 0) return ranges
  let off = 0
  for (const s of segs) {
    const len = s.text.length
    if (s.changed && len > 0) {
      const last = ranges[ranges.length - 1]
      if (last && last[1] === off) last[1] = off + len
      else ranges.push([off, off + len])
    }
    off += len
  }
  return ranges
}

/**
 * 把改动区间投影到语法 token 上：token 可能被区间从中间切开，故逐 token 扫描、
 * 在边界处拆分，得到「既是语法色、又带差异标记」的最小片段。
 * ranges 严格递增且互不重叠，用单向游标推进，整体 O(行宽)。
 */
export function markChangedTokens(tokens: Token[], ranges: Array<[number, number]>): StyledToken[] {
  if (ranges.length === 0) {
    return tokens.map((tk) => ({ text: tk.text, type: tk.type, changed: false }))
  }
  let ri = 0
  const chgAt = (i: number): boolean => {
    while (ri < ranges.length && i >= ranges[ri][1]) ri++
    return ri < ranges.length && i >= ranges[ri][0]
  }
  const out: StyledToken[] = []
  let base = 0
  for (const tk of tokens) {
    let pieceStart = 0
    let pieceChanged = chgAt(base)
    for (let k = 1; k < tk.text.length; k++) {
      const chg = chgAt(base + k)
      if (chg !== pieceChanged) {
        out.push({ text: tk.text.slice(pieceStart, k), type: tk.type, changed: pieceChanged })
        pieceStart = k
        pieceChanged = chg
      }
    }
    out.push({ text: tk.text.slice(pieceStart), type: tk.type, changed: pieceChanged })
    base += tk.text.length
  }
  return out
}
