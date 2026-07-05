#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

cd "$repo_root/ui"
npm ci
npm run build

cd "$repo_root"
cargo build -p cli
