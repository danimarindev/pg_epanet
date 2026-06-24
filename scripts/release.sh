#!/usr/bin/env bash
# release.sh — thin wrapper around the Rust xtask release tool
#
# Usage:
#   ./scripts/release.sh 0.1.0
#   ./scripts/release.sh 0.1.0 --create-github-release
#   ./scripts/release.sh 0.1.0 --github-release-only
#
# Equivalent:
#   cargo xtask release 0.1.0 [--create-github-release]

set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
exec cargo run --quiet --manifest-path "${ROOT}/xtask/Cargo.toml" -- release "$@"
