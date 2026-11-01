#!/usr/bin/env bash
set -euo pipefail

# FunPay Tools - скрипт установки в 1 команду
# Использование: curl -fsSL https://raw.githubusercontent.com/<REPO>/main/install.sh | bash

REPO="${GITHUB_REPO:-XaviersDev/FunPay-Desktop}"
BIN_NAME="funpay-tools-desktop"

echo "=============================================="
echo "    Установка FunPay Tools (Linux)           "
echo "=============================================="

ARCH="$(uname -m)"
if [ "$ARCH" != "x86_64" ]; then
    echo "Ошибка: поддерживается только архитектура x86_64 (ваша: $ARCH)" >&2
    exit 1
fi

# Проверка базовых зависимостей
check_dependencies() {
    local missing=()
    if ! ldconfig -p 2>/dev/null | grep -q "libwebkit2gtk-4.1.so"; then
        missing+=("webkit2gtk-4.1")
    fi
    if ! ldconfig -p 2>/dev/null | grep -q "libgtk-3.so"; then
        missing+=("gtk3")
    fi

    if [ ${#missing[@]} -gt 0 ]; then
        echo "Предупреждение: не найдены некоторые системные библиотеки: ${missing[*]}"
        echo "Для корректной работы установите их через ваш пакетный менеджер:"
        if command -v pacman &>/dev/null; then
            echo "  Arch/Manjaro: sudo pacman -S webkit2gtk-4.1 gtk3 libayatana-appindicator"
        elif command -v apt-get &>/dev/null; then
            echo "  Ubuntu/Debian: sudo apt update && sudo apt install -y libwebkit2gtk-4.1-0 libgtk-3-0 libayatana-appindicator3-1"
        elif command -v dnf &>/dev/null; then
            echo "  Fedora: sudo dnf install -y webkit2gtk4.1 gtk3 libayatana-appindicator"
        fi
        echo ""
    fi
}
check_dependencies

# Определение путей установки
if [ "$(id -u)" -eq 0 ]; then
    INSTALL_BIN="/usr/local/bin"
    INSTALL_APPS="/usr/share/applications"
    INSTALL_ICONS="/usr/share/icons/hicolor/256x256/apps"
else
    INSTALL_BIN="$HOME/.local/bin"
    INSTALL_APPS="$HOME/.local/share/applications"
    INSTALL_ICONS="$HOME/.local/share/icons/hicolor/256x256/apps"
fi

mkdir -p "$INSTALL_BIN" "$INSTALL_APPS" "$INSTALL_ICONS"

# Получение ссылки на последний релиз
echo "==> Поиск последнего релиза $REPO..."
RELEASE_JSON="$(curl -sSL "https://api.github.com/repos/$REPO/releases/latest" 2>/dev/null || true)"

DOWNLOAD_URL=""
if [ -n "$RELEASE_JSON" ]; then
    # Ищем бинарник или AppImage в assets
    DOWNLOAD_URL="$(echo "$RELEASE_JSON" | grep "browser_download_url" | grep -E "funpay-tools-desktop|FunPay_Tools.*\.AppImage" | head -n 1 | cut -d '"' -f 4 || true)"
fi

TARGET_FILE="$INSTALL_BIN/$BIN_NAME"

if [ -n "$DOWNLOAD_URL" ]; then
    echo "==> Загрузка $DOWNLOAD_URL..."
    curl -sSL -o "$TARGET_FILE" "$DOWNLOAD_URL"
else
    echo "Предупреждение: релиз в GitHub не найден. Если у вас локальный репозиторий, собираем из исходников..."
    if command -v cargo &>/dev/null; then
        cargo build --release
        cp target/release/funpay-tools-desktop "$TARGET_FILE"
    else
        echo "Ошибка: нет готового релиза и не установлен cargo для сборки." >&2
        exit 1
    fi
fi

chmod +x "$TARGET_FILE"

# Иконка
echo "==> Установка иконки..."
ICON_TARGET="$INSTALL_ICONS/funpay-tools.png"
if [ -f "ui/icons/logo256.png" ]; then
    cp "ui/icons/logo256.png" "$ICON_TARGET"
else
    curl -sSL -o "$ICON_TARGET" "https://raw.githubusercontent.com/$REPO/main/ui/icons/logo256.png" 2>/dev/null || true
fi

# Desktop Entry
echo "==> Регистрация ярлыка приложения..."
cat << EOF > "$INSTALL_APPS/funpay-tools.desktop"
[Desktop Entry]
Name=FunPay Tools
Comment=Утилиты для FunPay
Exec=$TARGET_FILE %u
Icon=funpay-tools
Terminal=false
Type=Application
Categories=Network;Utility;
StartupWMClass=funpay-tools
EOF

# Обновление кэша ярлыков
if command -v update-desktop-database &>/dev/null; then
    update-desktop-database "$INSTALL_APPS" 2>/dev/null || true
fi

# Проверка PATH
if [[ ":$PATH:" != *":$INSTALL_BIN:"* ]]; then
    echo ""
    echo "Заметка: каталог $INSTALL_BIN отсутствует в вашем PATH."
    echo "Добавьте строку в ваш ~/.bashrc или ~/.zshrc:"
    echo "  export PATH=\"\$HOME/.local/bin:\$PATH\""
fi

echo ""
echo "=============================================="
echo "    Установка успешно завершена!              "
echo "    Запуск: $TARGET_FILE                      "
echo "    Либо через меню приложений: FunPay Tools "
echo "=============================================="
