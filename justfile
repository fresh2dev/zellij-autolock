# ─────────────────────────────────────────────────────────────────────────────
# Tools
# ─────────────────────────────────────────────────────────────────────────────

git_cliff := '''
    uvx --from 'git-cliff==2.*' \
    -- git-cliff'''

# ─────────────────────────────────────────────────────────────────────────────
# Modules (present only when enabled for this project)
# ─────────────────────────────────────────────────────────────────────────────

import? 'rust.just'
import? 'python.just'
import? 'docs.just'

# ─────────────────────────────────────────────────────────────────────────────
# Default
# ─────────────────────────────────────────────────────────────────────────────

[positional-arguments]
default:
    @ just --choose

# ─────────────────────────────────────────────────────────────────────────────
# Core
# ─────────────────────────────────────────────────────────────────────────────

[positional-arguments]
changelog:
    {{ git_cliff }} --unreleased --prepend CHANGELOG.md

# Formatters are configured in .treefmt.toml.
[positional-arguments]
format *paths:
    treefmt "$@"
