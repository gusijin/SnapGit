import type { Commit } from '../types'

// 每行图谱的固定高度，必须与 LogView 中 .log-item 的高度一致
export const GRAPH_ROW_HEIGHT = 38
// 每条泳道占用的宽度
export const GRAPH_LANE_WIDTH = 14
// 图谱最多渲染的泳道数，历史过于嘈杂时收窄，避免挤占提交信息
const MAX_VISIBLE_LANES = 10

// 深浅主题都清晰可辨的中饱和度色板，按泳道复用
const LANE_COLORS = [
  '#4c8df6',
  '#f0883e',
  '#3fb950',
  '#a371f7',
  '#e5534b',
  '#d29922',
  '#39c5cf',
  '#db61a2'
]

export function laneColor(lane: number): string {
  return LANE_COLORS[((lane % LANE_COLORS.length) + LANE_COLORS.length) % LANE_COLORS.length]
}

export interface GraphLine {
  lane: number
  y1: number
  y2: number
  color: string
}

export interface GraphCurve {
  fromLane: number
  toLane: number
  color: string
}

export interface GraphRow {
  // 节点所在泳道；-1 表示超出可显示泳道数，只画一个占位点
  lane: number
  color: string
  lines: GraphLine[]
  curves: GraphCurve[]
}

export interface CommitGraph {
  rows: GraphRow[]
  // 需要渲染的泳道总数（已按 MAX_VISIBLE_LANES 截断）
  lanes: number
}

/**
 * 按提交顺序（新→旧）推导泳道布局。
 * 每条泳道记着「正在等待出现的提交 id」：遇到该提交时它就是节点所在泳道，
 * 其第一个父提交继承该泳道，多出来的父提交（合并）占用新泳道。
 */
export function computeCommitGraph(commits: Commit[]): CommitGraph {
  const lanes: (string | null)[] = []
  const rows: GraphRow[] = []
  const mid = GRAPH_ROW_HEIGHT / 2
  let laneCount = 1

  for (const commit of commits) {
    let lane = lanes.indexOf(commit.id)
    let startedHere = false
    if (lane === -1) {
      const hole = lanes.indexOf(null)
      if (hole === -1) {
        lane = lanes.length
        lanes.push(commit.id)
      } else {
        lane = hole
        lanes[hole] = commit.id
      }
      startedHere = true
    }

    const before = lanes.slice()
    const after = before.slice()
    const parents = commit.parents ?? []
    const parentLanes: number[] = []

    if (parents.length === 0) {
      after[lane] = null
    } else {
      // 第一个父提交：若已在别的泳道上等待，说明分支在此汇入，本泳道结束
      let target = after.indexOf(parents[0])
      if (target === -1) {
        after[lane] = parents[0]
        target = lane
      } else if (target !== lane) {
        after[lane] = null
      }
      parentLanes.push(target)

      // 合并提交的其余父提交：复用空泳道，没有空位就向右新开一条
      for (const parent of parents.slice(1)) {
        let target2 = after.indexOf(parent)
        if (target2 === -1) {
          let hole = -1
          for (let k = lane + 1; k < after.length; k++) {
            if (after[k] === null) {
              hole = k
              break
            }
          }
          if (hole === -1) {
            hole = after.length
            after.push(parent)
          } else {
            after[hole] = parent
          }
          target2 = hole
        }
        parentLanes.push(target2)
      }
    }

    // 修剪末尾空泳道，避免图谱宽度无限增长
    while (after.length > 0 && after[after.length - 1] === null) {
      after.pop()
    }
    lanes.splice(0, lanes.length, ...after)

    const lines: GraphLine[] = []
    const curves: GraphCurve[] = []

    // 贯穿本行的其它泳道
    for (let j = 0; j < before.length; j++) {
      if (j === lane || before[j] === null) continue
      lines.push({ lane: j, y1: 0, y2: GRAPH_ROW_HEIGHT, color: laneColor(j) })
      laneCount = Math.max(laneCount, j + 1)
    }

    // 节点上方：本泳道从上一行延续下来
    if (!startedHere) {
      lines.push({ lane, y1: 0, y2: mid, color: laneColor(lane) })
    }

    // 节点下方：垂直接到父提交，或曲线汇入已有的父提交泳道
    for (const target of parentLanes) {
      if (target === lane) {
        lines.push({ lane, y1: mid, y2: GRAPH_ROW_HEIGHT, color: laneColor(lane) })
      } else {
        curves.push({ fromLane: lane, toLane: target, color: laneColor(target) })
      }
      laneCount = Math.max(laneCount, target + 1)
    }

    rows.push({
      lane: Math.min(lane, MAX_VISIBLE_LANES - 1),
      color: laneColor(lane),
      lines,
      curves
    })
  }

  return { rows, lanes: Math.min(Math.max(laneCount, 1), MAX_VISIBLE_LANES) }
}
