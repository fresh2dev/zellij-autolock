default:
    @ just --choose

# Idempotent toolchain setup; slow enough to keep out of `build`.
bootstrap:
    rustup toolchain install
    rustup target add wasm32-wasip1
    cargo fetch

[positional-arguments]
build *args:
    cargo build $@

# Tests run natively on the host target: the default build target is wasm
# (see .cargo/config.toml), which has no test runner installed.
[positional-arguments]
test *args:
    cargo test --target "$(rustc -vV | sed -n 's/^host: //p')" "$@"

clear-cache:
    rm -rf ~/.cache/zellij ~/Library/Caches/org.Zellij-Contributors.Zellij

install: bootstrap clear-cache (build "--release")
    mkdir -p "${ZELLIJ_CONFIG_DIR:-$HOME/.config/zellij}/plugins" \
    && cp target/wasm32-wasip1/release/zellij-autolock.wasm "${ZELLIJ_CONFIG_DIR:-$HOME/.config/zellij}/plugins/"
