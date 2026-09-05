/**
 * 智能锚定滚动同步（Beyond Compare 风格）
 * --------------------------------------------------
 * 在「逐行 1:1 对齐」的差异对比里，某些行只有一侧有内容（新增行左侧空、删除行右侧空）。
 * 朴素做法把两侧 scrollTop 直接 1:1 拷贝，导致空白的一侧被带着一起滚、匹配点错位。
 *
 * 这里改成「锚定到相同行」：
 *  - anchorFlags[i] === true 表示该行两侧都有内容（context / modified / eq / mod），可作对齐锚点。
 *  - 滚动某侧时，找到「当前滚动位置之上最近的锚点行 a」，把对端对齐到同一行（含行内偏移）。
 *  - 若当前正处于某段「空白 stretch」（两侧无共同内容）内，对端保持在上一个锚点附近不动，
 *    直到滚到下一个有共同代码的行（锚点）才重新对齐 —— 即「拉到一样的代码才同步滚动」。
 *  - 完全没有锚点（如全新文件全是新增、整栏为空）时回退为 1:1，避免卡死。
 */
export function computeAnchoredScrollTop(
  sourceTop: number,
  anchorFlags: boolean[],
  rowHeight: number,
): number {
  const n = anchorFlags.length
  if (n === 0 || rowHeight <= 0) return sourceTop
  const topLine = Math.floor(sourceTop / rowHeight)

  // 当前可见行之上最近的锚点
  let a = -1
  for (let i = Math.min(topLine, n - 1); i >= 0; i--) {
    if (anchorFlags[i]) {
      a = i
      break
    }
  }
  // 若上方无锚点，取下方第一个锚点
  if (a < 0) {
    for (let i = Math.max(topLine, 0); i < n; i++) {
      if (anchorFlags[i]) {
        a = i
        break
      }
    }
  }
  // 完全没有锚点 → 1:1 回退
  if (a < 0) return sourceTop

  const offset = sourceTop - topLine * rowHeight
  return a * rowHeight + Math.max(0, Math.min(rowHeight, offset))
}
