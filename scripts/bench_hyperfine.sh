#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [[ -n "${RUNS+x}" ]]; then
    echo "error: RUNS is retired; use adaptive --min-runs sampling" >&2
    exit 2
fi
exec python3 -B "${SCRIPT_DIR}/bench_comparison.py" "$@"
