default:
    @ just --choose

[positional-arguments]
build *args:
    cargo build $@

clear-cache:
    rm -rf ~/.cache/zellij ~/Library/Caches/org.Zellij-Contributors.Zellij

install: clear-cache (build "--release")
    mkdir -p "${ZELLIJ_CONFIG_DIR:-$HOME/.config/zellij}/plugins" \
    && cp target/wasm32-wasip1/release/zellij-autolock.wasm "${ZELLIJ_CONFIG_DIR:-$HOME/.config/zellij}/plugins/"
