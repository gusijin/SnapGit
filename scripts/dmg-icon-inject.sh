#!/usr/bin/env bash
# scripts/dmg-icon-inject.sh
# 把自定义 Finder 图标注入到 macOS dmg 文件里。
#
# 工作原理（macOS 上的官方 SetFile + Rez 套路）：
#   1. dmg 转 UDRW（read-write） → hdiutil convert
#   2. 挂载 → hdiutil attach -readwrite
#   3. 在挂载点根目录创建 Icon\r 文件（HFS+ resource fork 标记），
#      并用 Rez 把 icon.icns 作为 icns 资源注入到 Icon\r
#   4. SetFile -a C 挂载点（告诉 Finder/dmg："这个卷有自定义图标"）
#   5. SetFile -a V 隐藏 Icon\r（用户不应在 Finder 里看到这个文件）
#   6. 卸载 → hdiutil detach
#   7. dmg 转回 UDZO（read-only compressed） → hdiutil convert
#      卸载 dmg 时 macOS 会自动把挂载点的 custom icon 同步到
#      dmg 文件本身的 Finder metadata，所以最终 dmg 在 Finder 里
#      （不挂载）会显示 SnapGit 图标，而不是默认的黄色卷宗图标。
#
# 注意：dmg 文件名（SnapGit_<ver>_<arch>.dmg）会被原位替换，
#       体积几乎不变（UDZO 压缩后）。
#
# 依赖：macOS + Xcode Command Line Tools
#       - hdiutil （系统自带）
#       - Rez      （xcrun --find Rez）
#       - SetFile  （xcrun --find SetFile）
#
# 用法：
#   ./scripts/dmg-icon-inject.sh <icon.icns> <dmg> [<dmg>...]
#
# 退出码：
#   0  成功
#   64 用法错误（参数缺失）
#   66 图标文件不存在
#   69 缺少 Xcode CLT
#   70 挂载失败
#   71 注入失败
#   72 转回 UDZO 失败
set -euo pipefail

if [[ $# -lt 2 ]]; then
  echo "用法: $0 <icon.icns> <dmg> [<dmg>...]" >&2
  exit 64
fi

ICON_SRC="$1"; shift

if [[ ! -f "$ICON_SRC" ]]; then
  echo "图标文件不存在: $ICON_SRC" >&2
  exit 66
fi

# 找 Xcode CLT 工具
REZ=$(xcrun --find Rez 2>/dev/null || command -v Rez || true)
SETFILE=$(xcrun --find SetFile 2>/dev/null || command -v SetFile || true)

if [[ -z "$REZ" || -z "$SETFILE" ]]; then
  echo "需要 Xcode Command Line Tools (Rez + SetFile)" >&2
  echo "安装: xcode-select --install" >&2
  exit 69
fi

echo "[dmg-icon] Rez    = $REZ"
echo "[dmg-icon] SetFile= $SETFILE"
echo "[dmg-icon] icon   = $ICON_SRC"

# Icon\r 文件名（\r 是 CR=0x0d，HFS+ resource fork 标记）
# 写入到这个特殊文件名的 resource fork 数据，会被 macOS 当成卷宗/dmg 的图标资源
ICON_NAME="Icon$(printf '\r')"

cleanup() {
  if [[ -n "${MOUNT_DIR:-}" && -d "$MOUNT_DIR" ]]; then
    hdiutil detach "$MOUNT_DIR" >/dev/null 2>&1 || true
  fi
  if [[ -n "${TMPDIR_ICON:-}" && -d "${TMPDIR_ICON}" ]]; then
    rm -rf "${TMPDIR_ICON}"
  fi
}
trap cleanup EXIT

for DMG in "$@"; do
  echo
  echo "[dmg-icon] 处理: $DMG"

  if [[ ! -f "$DMG" ]]; then
    echo "  ! 文件不存在，跳过" >&2
    continue
  fi

  TMPDIR_ICON=$(mktemp -d -t dmgicon.XXXXXX)
  RW_DMG="$TMPDIR_ICON/rw.dmg"
  FINAL_DMG="$TMPDIR_ICON/final.dmg"
  MOUNT_DIR=""

  # 1. 转 UDRW
  echo "  [1/6] dmg → UDRW ..."
  if ! hdiutil convert "$DMG" -format UDRW -o "$RW_DMG" >/dev/null; then
    echo "  ! UDRW 转换失败" >&2
    exit 71
  fi

  # 2. 挂载
  echo "  [2/6] 挂载 read-write ..."
  # hdiutil attach 输出形如:
  #   /dev/disk5s1        	Apple_HFS                      	/Volumes/SnapGit Installer
  ATTACH_OUT=$(hdiutil attach -readwrite -noverify -noautoopen -nobrowse "$RW_DMG" 2>&1) || {
    echo "  ! 挂载失败：$ATTACH_OUT" >&2
    exit 70
  }
  MOUNT_DIR=$(echo "$ATTACH_OUT" | awk '/Apple_HFS|Apple_APFS/ {print $NF; exit}')

  if [[ -z "$MOUNT_DIR" || ! -d "$MOUNT_DIR" ]]; then
    echo "  ! 解析挂载点失败" >&2
    echo "$ATTACH_OUT" >&2
    exit 70
  fi
  echo "         挂载点: $MOUNT_DIR"

  # 3. 写入 Icon\r（注入 icns resource fork 数据）
  echo "  [3/6] 写入 Icon\r ..."
  ICON_DEST="$MOUNT_DIR/$ICON_NAME"
  REZ_SRC="$TMPDIR_ICON/icon.r"

  # Rez 是资源编译器，只吃 .r 源文件（文本），不能直接吃 .icns 二进制。
  # 这里先把 .icns 二进制 hex 化成一份 .r 源，再编译进 Icon\r 的 resource fork。
  # 优先用 python3（macos runner 预装），否则用 BSD od 兜底。
  if command -v python3 >/dev/null 2>&1; then
    python3 - "$ICON_SRC" "$REZ_SRC" <<'PY'
import sys
src, out = sys.argv[1], sys.argv[2]
data = open(src, 'rb').read()
hexstr = data.hex()  # 连续 hex 字符串
with open(out, 'w') as f:
    f.write("data 'icns' (128) {\n")
    # 每 16 字节（32 个 hex 字符）一行，行内每字节空一格，便于 Rez 解析
    for i in range(0, len(hexstr), 32):
        chunk = hexstr[i:i + 32]
        spaced = ' '.join(chunk[j:j + 2] for j in range(0, len(chunk), 2))
        f.write('  $"' + spaced + '"\n')
    f.write("};\n")
PY
  else
    {
      echo "data 'icns' (128) {"
      od -An -v -tx1 "$ICON_SRC" | tr -s ' ' | sed 's/^ //' | awk '{ printf "  $\"%s\"\n", $0 }'
      echo "};"
    } > "$REZ_SRC"
  fi

  # 确保 Icon\r 文件存在（Rez -o 会写它的 resource fork）
  : > "$ICON_DEST"
  if ! "$REZ" "$REZ_SRC" -o "$ICON_DEST"; then
    echo "  ! Rez 注入失败" >&2
    exit 71
  fi

  # 4. SetFile -a C 挂载点（标记卷宗/dmg 拥有 custom icon）
  echo "  [4/6] SetFile -a C ..."
  "$SETFILE" -a C "$MOUNT_DIR"

  # 5. SetFile -a V 隐藏 Icon\r（用户不应看到这个文件）
  echo "  [5/6] SetFile -a V (隐藏 Icon\r) ..."
  "$SETFILE" -a V "$ICON_DEST"

  # 6. 卸载 → 转回 UDZO
  echo "  [6/6] 卸载 + 转 UDZO ..."
  hdiutil detach "$MOUNT_DIR" >/dev/null
  MOUNT_DIR=""  # 已卸载，避免 trap 重做

  if ! hdiutil convert "$RW_DMG" -format UDZO -imagekey zlib-level=9 -o "$FINAL_DMG" >/dev/null; then
    echo "  ! UDZO 转换失败" >&2
    exit 72
  fi

  # 7. 替换原 dmg
  mv "$FINAL_DMG" "$DMG"

  rm -rf "$TMPDIR_ICON"
  TMPDIR_ICON=""

  echo "  ✓ 完成: $DMG"
done

echo
echo "[dmg-icon] 全部完成 ✓"