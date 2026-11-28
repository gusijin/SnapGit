/**
 * 编辑窗口 textarea 滚动条的光标修正。
 *
 * Chromium / WebKit 的 ::-webkit-scrollbar 热区属于滚动容器自身，鼠标停上去时
 * 沿用元素的 cursor（textarea 默认是 text），于是滚动条上也会显示输入光标 I-beam。
 * 这里在 mousemove 时把「滚动条槽」区域的 cursor 切成 default，文本区保持 text。
 *
 * 槽宽/高优先用 offsetWidth-clientWidth / offsetHeight-clientHeight 动态取；
 * macOS overlay 滚动条不占布局（差值为 0）时，按项目统一的 10px 规格兜底，
 * 保证滚动条浮层所在的最右 / 最下 10px 也不会显示输入光标。
 * 本项目 textarea 为 border:none，差值即滚动条宽高（有边框时会多算边框宽度，
 * 最多让靠边 1~2px 的代码区显示箭头，不影响编辑）。
 *
 * 配套：滚动条宿主的外层容器需显式 cursor: default，覆盖「滚动条继承父级
 * cursor」的浏览器行为；textarea 自身显式 cursor: text，保证文本区仍是输入光标。
 */
export function updateTextareaCursor(event: MouseEvent): void {
  const el = event.currentTarget as HTMLTextAreaElement | null
  if (!el) return

  // 与两个编辑组件统一的 ::-webkit-scrollbar 规格（width/height: 10px）
  const FALLBACK_BAR = 10
  const rect = el.getBoundingClientRect()
  // 经典滚动条占布局时 offset/client 差值即槽宽；overlay 滚动条不占布局（差值为 0），
  // 此时内容可纵滚仍按右侧 10px 视为热区（滚动条浮在该处）；横向滚动条是常驻的，恒按 10px。
  const vBar = Math.max(
    el.offsetWidth - el.clientWidth,
    el.scrollHeight > el.clientHeight ? FALLBACK_BAR : 0,
  )
  const hBar = Math.max(el.offsetHeight - el.clientHeight, FALLBACK_BAR)
  const onScrollbar =
    (vBar > 0 && event.clientX >= rect.right - vBar) ||
    (hBar > 0 && event.clientY >= rect.bottom - hBar)

  const next = onScrollbar ? 'default' : 'text'
  // 只在状态变化时写 inline style，避免每次 mousemove 都触发样式重算
  if (el.style.cursor !== next) el.style.cursor = next
}
