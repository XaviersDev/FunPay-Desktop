#!/usr/bin/env bash
set -euo pipefail

# tools/build-appimage.sh - сборка FunPay Tools в портативный AppImage
PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_DIR"

echo "==> Сборка релизного бинарника..."
cargo build --release

BIN_SRC="$PROJECT_DIR/target/release/funpay-tools-desktop"
if [ ! -f "$BIN_SRC" ]; then
    echo "Ошибка: исполняемый файл $BIN_SRC не найден!" >&2
    exit 1
fi

BUILD_DIR="$PROJECT_DIR/target/appimage"
APP_DIR="$BUILD_DIR/FunPayTools.AppDir"
OUT_APPIMAGE="$PROJECT_DIR/target/FunPay_Tools-x86_64.AppImage"

rm -rf "$BUILD_DIR"
mkdir -p "$APP_DIR/usr/bin"
mkdir -p "$APP_DIR/usr/share/icons/hicolor/256x256/apps"
mkdir -p "$APP_DIR/usr/share/applications"

echo "==> Подготовка AppDir..."
cp "$BIN_SRC" "$APP_DIR/usr/bin/funpay-tools-desktop"
chmod +x "$APP_DIR/usr/bin/funpay-tools-desktop"

# Иконки
cp "$PROJECT_DIR/ui/icons/logo256.png" "$APP_DIR/funpay-tools.png"
cp "$PROJECT_DIR/ui/icons/logo256.png" "$APP_DIR/usr/share/icons/hicolor/256x256/apps/funpay-tools.png"

# Desktop файл
cat << 'EOF' > "$APP_DIR/funpay-tools.desktop"
[Desktop Entry]
Name=FunPay Tools
Comment=Утилиты для FunPay
Exec=funpay-tools-desktop %u
Icon=funpay-tools
Terminal=false
Type=Application
Categories=Network;Utility;
StartupWMClass=funpay-tools
EOF

cp "$APP_DIR/funpay-tools.desktop" "$APP_DIR/usr/share/applications/funpay-tools.desktop"

# AppRun
cat << 'EOF' > "$APP_DIR/AppRun"
#!/bin/sh
set -e
HERE="$(dirname "$(readlink -f "${0}")")"
export PATH="${HERE}/usr/bin:${PATH}"
export XDG_DATA_DIRS="${HERE}/usr/share:${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"
exec "${HERE}/usr/bin/funpay-tools-desktop" "$@"
EOF
chmod +x "$APP_DIR/AppRun"

# Загрузка / запуск appimagetool
APPIMAGETOOL="$BUILD_DIR/appimagetool"
if ! command -v appimagetool &>/dev/null; then
    echo "==> Скачивание appimagetool..."
    curl -sSL -o "$APPIMAGETOOL" "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage"
    chmod +x "$APPIMAGETOOL"
    TOOL_CMD="$APPIMAGETOOL --appimage-extract-and-run"
else
    TOOL_CMD="appimagetool"
fi

echo "==> Генерация AppImage..."
ARCH=x86_64 $TOOL_CMD "$APP_DIR" "$OUT_APPIMAGE"

echo "==> Готово: $OUT_APPIMAGE"
ls -lh "$OUT_APPIMAGE"
