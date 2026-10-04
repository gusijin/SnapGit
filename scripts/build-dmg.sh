#!/usr/bin/env bash
# scripts/build-dmg.sh
# 用 `hdiutil create -srcfolder` 生成 dmg —— 无需 attach/detach 挂载，
# 因此在 CI runner（无 GUI 会话）上稳定可靠，避免 Tauri 内置 create-dmg fork
# 在 hdiutil 挂载/设备节点解析环节失败（failed to run bundle_dmg.sh）。
#
# 依赖：macOS + 系统自带 hdiutil（无需 Xcode CLT 的 Rez/SetFile）
#
# 用法：
#   ./scripts/build-dmg.sh <SnapGit.app> <output.dmg> [volname]
#
# 退出码：
#   0  成功
#   64 用法错误
#   66 App 不存在
set -euo pipefail

if [[ $# -lt 2 ]]; then
  echo "用法: $0 <SnapGit.app> <output.dmg> [volname]" >&2
  exit 64
fi

APP_SRC="$1"
DMG_OUT="$2"
VOLNAME="${3:-SnapGit}"

if [[ ! -d "$APP_SRC" ]]; then
  echo "App 不存在: $APP_SRC" >&2
  exit 66
fi

STAGING="$(mktemp -d -t snapgit-dmg.XXXXXX)"
trap 'rm -rf "$STAGING"' EXIT

cp -R "$APP_SRC" "$STAGING/"
# 拖拽安装用的 Applications 软链（Finder 里显示「应用程序」文件夹）
ln -s /Applications "$STAGING/Applications"

echo "[build-dmg] staging: $STAGING"
echo "[build-dmg] volname: $VOLNAME"
echo "[build-dmg] output : $DMG_OUT"

# -srcfolder 直接从文件夹生成 dmg，内核态完成，不需要用户态挂载，
# 从根本上避开 CI 上 hdiutil attach 挂载失败的问题。
hdiutil create \
  -volname "$VOLNAME" \
  -srcfolder "$STAGING" \
  -ov \
  -format UDZO \
  "$DMG_OUT"

echo "[build-dmg] 完成: $(ls -lh "$DMG_OUT" | awk '{print $5, $9}')"
