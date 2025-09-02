# ─────────────────────────────────────────────────────────────────────────────
# Tools
# ─────────────────────────────────────────────────────────────────────────────

# The default build target is wasm (see .cargo/config.toml), which has no test
# runner installed, so tests run natively on the host target. Expanded by the
# shell when a recipe runs, not by just on every invocation.
host_target := '"$(rustc -vV | sed -n "s/^host: //p")"'

wasm := 'target/wasm32-wasip1/release/zellij-autolock.wasm'
plugin_dir := '"${ZELLIJ_CONFIG_DIR:-$HOME/.config/zellij}/plugins"'

# ─────────────────────────────────────────────────────────────────────────────
# Build & Package
# ─────────────────────────────────────────────────────────────────────────────

[positional-arguments]
build *args:
    cargo build "$@"

# Zellij caches loaded plugins; clear it or the old wasm keeps running.
clear-cache:
    rm -rf ~/.cache/zellij ~/Library/Caches/org.Zellij-Contributors.Zellij

install: bootstrap clear-cache (build "--release")
    mkdir -p {{ plugin_dir }}
    cp {{ wasm }} {{ plugin_dir }}/

# ─────────────────────────────────────────────────────────────────────────────
# Dependencies
# ─────────────────────────────────────────────────────────────────────────────

# Idempotent toolchain setup; slow enough to keep out of `build`.
bootstrap:
    rustup toolchain install
    rustup target add wasm32-wasip1
    cargo fetch

# ─────────────────────────────────────────────────────────────────────────────
# Quality
# ─────────────────────────────────────────────────────────────────────────────

# `format` is the treefmt recipe in the core justfile.
[positional-arguments]
fix *args: && format
    cargo clippy --fix --allow-dirty --allow-staged --all-targets "$@"

# Same flags as CI: any warning, in the tests too, fails.
[positional-arguments]
lint *args:
    RUSTFLAGS=-Dwarnings cargo clippy --all-targets "$@"

alias check := lint

[positional-arguments]
check-types *args:
    cargo check --all-targets "$@"

alias typecheck := check-types

[positional-arguments]
test *args:
    cargo test --target {{ host_target }} "$@"

# The local equivalent of the CI jobs (fmt, clippy, test, build), to run before
# pushing. `--locked` fails if Cargo.lock is out of date, as it does in CI.
check-ci: && lint (test "--locked") (build "--release" "--locked")
    cargo fmt --all --check

clean:
    cargo clean
