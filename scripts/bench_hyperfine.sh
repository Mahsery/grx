#!/usr/bin/env bash
# scripts/bench_hyperfine.sh
# Comprehensive, statistically rigorous benchmark suite using hyperfine.
# Compares grx against ripgrep across local codebases and large-scale repos.

set -euo pipefail

# Ensure hyperfine is installed
if ! command -v hyperfine >/dev/null 2>&1; then
    echo "error: hyperfine is not installed or not in PATH." >&2
    echo "Install hyperfine via your package manager or cargo: cargo install hyperfine" >&2
    exit 1
fi

# Ensure grx and rg are available
if ! command -v grx >/dev/null 2>&1; then
    echo "error: grx is not in PATH. Run: cargo install --path . --force" >&2
    exit 1
fi

if ! command -v rg >/dev/null 2>&1; then
    echo "warning: ripgrep (rg) not found. Comparisons will be skipped." >&2
    HAS_RG=false
else
    HAS_RG=true
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
LINUX_TREE="${1:-${LINUX_TREE:-}}"

echo "================================================================================"
echo " grx vs ripgrep — Hyperfine Benchmark Suite"
echo " Date: $(date -Iseconds)"
echo " Hyperfine: $(hyperfine --version)"
echo " grx: $(grx --version)"
[ "$HAS_RG" = true ] && echo " ripgrep: $(rg --version | head -n 1)"
echo "================================================================================"

# ------------------------------------------------------------------------------
# SUITE 1: Local Repository Traversal & Search (Warm Tree)
# ------------------------------------------------------------------------------
echo ""
echo "--------------------------------------------------------------------------------"
echo " [Suite 1] Local Repository Searches (grx/src/)"
echo "--------------------------------------------------------------------------------"

echo ">> 1.1 Literal Identifier ('AdaptiveReader' in src/)"
if [ "$HAS_RG" = true ]; then
    hyperfine --shell=none --warmup 5 \
        "grx AdaptiveReader ${REPO_DIR}/src" \
        "rg AdaptiveReader ${REPO_DIR}/src"
else
    hyperfine --shell=none --warmup 5 "grx AdaptiveReader ${REPO_DIR}/src"
fi

echo ""
echo ">> 1.2 Common Keyword ('unsafe' in src/)"
if [ "$HAS_RG" = true ]; then
    hyperfine --shell=none --warmup 5 \
        "grx unsafe ${REPO_DIR}/src" \
        "rg unsafe ${REPO_DIR}/src"
else
    hyperfine --shell=none --warmup 5 "grx unsafe ${REPO_DIR}/src"
fi

echo ""
echo ">> 1.3 Boolean Query ('auth' AND 'token' in src/)"
if [ "$HAS_RG" = true ]; then
    hyperfine --shell=none --warmup 5 \
        "grx auth AND token ${REPO_DIR}/src" \
        "rg 'auth.*token|token.*auth' ${REPO_DIR}/src"
else
    hyperfine --shell=none --warmup 5 "grx auth AND token ${REPO_DIR}/src"
fi

echo ""
echo ">> 1.4 Fuzzy Token Permutation ('%%from_ptr_err' in src/)"
hyperfine --shell=none --warmup 5 \
    "grx %%from_ptr_err ${REPO_DIR}/src"

echo ""
echo ">> 1.5 Proximity Line Window ('unsafe' near:5,safety in src/)"
hyperfine --shell=none --warmup 5 \
    "grx unsafe near:5,safety ${REPO_DIR}/src"

# ------------------------------------------------------------------------------
# SUITE 2: Large Repository Traversal (Linux Kernel Tree)
# ------------------------------------------------------------------------------
if [ -d "${LINUX_TREE}" ]; then
    echo ""
    echo "--------------------------------------------------------------------------------"
    echo " [Suite 2] Large Repository: Linux Kernel Tree (~168,000 files, ~2.66 GB source)"
    echo " Target: ${LINUX_TREE}"
    echo "--------------------------------------------------------------------------------"

    echo ">> 2.1 Literal Symbol in C Source ('register_filesystem' :c)"
    if [ "$HAS_RG" = true ]; then
        hyperfine --warmup 1 -m 3 \
            "grx 'register_filesystem' :c ${LINUX_TREE}" \
            "rg -t c 'register_filesystem' ${LINUX_TREE}"
    else
        hyperfine --warmup 1 -m 3 "grx 'register_filesystem' :c ${LINUX_TREE}"
    fi

    echo ""
    echo ">> 2.2 Boolean AND Expression ('EXPORT_SYMBOL' AND 'GPL' :c)"
    if [ "$HAS_RG" = true ]; then
        hyperfine --warmup 1 -m 3 \
            "grx 'EXPORT_SYMBOL' AND 'GPL' :c ${LINUX_TREE}" \
            "rg 'EXPORT_SYMBOL.*GPL|GPL.*EXPORT_SYMBOL' -t c ${LINUX_TREE}"
    else
        hyperfine --warmup 1 -m 3 "grx 'EXPORT_SYMBOL' AND 'GPL' :c ${LINUX_TREE}"
    fi

    echo ""
    echo ">> 2.3 Shell-Safe Wildcard ('mutex..lock' :c)"
    if [ "$HAS_RG" = true ]; then
        hyperfine --warmup 1 -m 3 \
            "grx 'mutex..lock' :c ${LINUX_TREE}" \
            "rg -t c 'mutex.*lock' ${LINUX_TREE}"
    else
        hyperfine --warmup 1 -m 3 "grx 'mutex..lock' :c ${LINUX_TREE}"
    fi

    echo ""
    echo ">> 2.4 Fuzzy Token Permutation ('%%spin_lock_irqsave' :c)"
    if [ "$HAS_RG" = true ]; then
        hyperfine --warmup 1 -m 3 \
            "grx '%%spin_lock_irqsave' :c ${LINUX_TREE}" \
            "rg -t c 'spin_lock_irqsave' ${LINUX_TREE}"
    else
        hyperfine --warmup 1 -m 3 "grx '%%spin_lock_irqsave' :c ${LINUX_TREE}"
    fi

    echo ""
    echo ">> 2.5 Proximity Search ('spin_lock' near:3,spin_unlock :c)"
    hyperfine --warmup 1 -m 3 \
        "grx 'spin_lock' near:3,spin_unlock :c ${LINUX_TREE}"

else
    echo ""
    echo "Note: Linux kernel tree not found at ${LINUX_TREE}. Pass path as argument: $0 /path/to/linux"
fi

echo ""
echo "================================================================================"
echo " Hyperfine Benchmark Suite Complete!"
echo "================================================================================"
