#!/usr/bin/env bash

set -euo pipefail

SOURCE_PNG="icon.png"
ICONS_DIR="assets/icons"
LINUX_APP_NAME="opencode-ui.png"

MAC_ICONSET="$ICONS_DIR/macos/AppIcon.iconset"
LINUX_PREFIX="$ICONS_DIR/linux/hicolor"
WIN_ICO="$ICONS_DIR/windows/app.ico"

MAC_SIZES=(16 32 128 256 512)
LINUX_SIZES=(16 24 32 48 64 128 256 512)

if [[ ! -f "$SOURCE_PNG" ]]; then
  echo "Source icon not found: $SOURCE_PNG"
  exit 1
fi

command -v sips >/dev/null || { echo "sips is required on macOS"; exit 1; }
command -v iconutil >/dev/null || { echo "iconutil is required on macOS"; exit 1; }
command -v python3 >/dev/null || { echo "python3 is required to generate .ico"; exit 1; }

mkdir -p "$MAC_ICONSET" "$LINUX_PREFIX" "$ICONS_DIR/windows"

create_resized_png() {
  local source="$1"
  local target="$2"
  local size="$3"

  sips -z "$size" "$size" "$source" --out "$target" >/dev/null
}

# macOS iconset
for size in "${MAC_SIZES[@]}"; do
  create_resized_png "$SOURCE_PNG" "$MAC_ICONSET/icon_${size}x${size}.png" "$size"
  x2=$((size * 2))
  create_resized_png "$SOURCE_PNG" "$MAC_ICONSET/icon_${size}x${size}@2x.png" "$x2"
done

rm -f "$ICONS_DIR/macos/AppIcon.icns"
iconutil -c icns "$MAC_ICONSET" -o "$ICONS_DIR/macos/AppIcon.icns"

# Linux hicolor sizes
for size in "${LINUX_SIZES[@]}"; do
  dir="$LINUX_PREFIX/${size}x${size}/apps"
  mkdir -p "$dir"
  create_resized_png "$SOURCE_PNG" "$dir/$LINUX_APP_NAME" "$size"
done

# Windows ICO
python3 - <<PY
from PIL import Image
src = Image.open("$SOURCE_PNG").convert("RGBA")
sizes = [(16,16), (24,24), (32,32), (48,48), (64,64), (128,128), (256,256)]
src.save(
    "$WIN_ICO",
    format="ICO",
    sizes=sizes,
)
PY

echo "Generated:"
echo "- $ICONS_DIR/macos/AppIcon.icns"
echo "- $ICONS_DIR/linux/hicolor/*/apps/$LINUX_APP_NAME"
echo "- $ICONS_DIR/windows/app.ico"
