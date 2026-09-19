#!/usr/bin/env python3
"""
Benchmark comparison script evaluating grx, ripgrep (rg), and ugrep
across the full Linux kernel tree (168,000+ files, 2.66+ GB source).
Measures wall-clock time, user CPU time, system CPU time, and match accuracy.
"""

import os
import sys
import time
import uuid
import resource
import subprocess

LINUX_TREE = sys.argv[1] if len(sys.argv) > 1 else os.environ.get("LINUX_TREE", "")

def run_bench(cmd, iterations=3, warmup=True):
    """Run command multiple times and record wall, user, and sys times."""
    if warmup:
        subprocess.run(cmd, shell=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    wall_times = []
    user_times = []
    sys_times = []
    last_stdout = b""
    last_rc = 0

    for _ in range(iterations):
        u0 = resource.getrusage(resource.RUSAGE_CHILDREN).ru_utime
        s0 = resource.getrusage(resource.RUSAGE_CHILDREN).ru_stime
        t0 = time.perf_counter()

        p = subprocess.run(cmd, shell=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)

        t1 = time.perf_counter()
        u1 = resource.getrusage(resource.RUSAGE_CHILDREN).ru_utime
        s1 = resource.getrusage(resource.RUSAGE_CHILDREN).ru_stime

        wall_times.append((t1 - t0) * 1000.0)
        user_times.append((u1 - u0) * 1000.0)
        sys_times.append((s1 - s0) * 1000.0)
        last_stdout = p.stdout
        last_rc = p.returncode

    avg_wall = sum(wall_times) / len(wall_times)
    min_wall = min(wall_times)
    avg_user = sum(user_times) / len(user_times)
    avg_sys = sum(sys_times) / len(sys_times)
    line_count = len(last_stdout.splitlines()) if last_stdout else 0

    return {
        "min_wall_ms": min_wall,
        "avg_wall_ms": avg_wall,
        "avg_user_ms": avg_user,
        "avg_sys_ms": avg_sys,
        "lines": line_count,
        "rc": last_rc,
    }

def run_ttfm(cmd_list, needle):
    """Measure Time-To-First-Match (TTFM) in milliseconds."""
    t0 = time.perf_counter()
    p = subprocess.Popen(cmd_list, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1)
    ttfm = None
    first_match = ""
    try:
        for line in p.stdout:
            if needle in line:
                ttfm = (time.perf_counter() - t0) * 1000.0
                first_match = line.strip()
                break
    finally:
        p.terminate()
        try:
            p.wait(timeout=1)
        except subprocess.TimeoutExpired:
            p.kill()

    return ttfm, first_match

def print_table_header():
    print(f"{'Engine':<16} | {'Min Wall':>10} | {'Avg Wall':>10} | {'User CPU':>10} | {'Sys CPU':>10} | {'Matches':>8}")
    print("-" * 75)

def print_row(name, res):
    print(f"{name:<16} | {res['min_wall_ms']:8.2f} ms | {res['avg_wall_ms']:8.2f} ms | {res['avg_user_ms']:8.2f} ms | {res['avg_sys_ms']:8.2f} ms | {res['lines']:>8}")

def main():
    if not LINUX_TREE or not os.path.isdir(LINUX_TREE):
        print("Usage: python3 scripts/bench_linux_tree.py <path_to_linux_tree>", file=sys.stderr)
        print("   or: LINUX_TREE=/path/to/linux python3 scripts/bench_linux_tree.py", file=sys.stderr)
        sys.exit(1)

    print("=" * 80)
    print("  LINUX KERNEL TREE BENCHMARK (168,555 files, ~2.66 GB source)")
    print(f"  Target: {LINUX_TREE}")
    print("=" * 80)

    # -------------------------------------------------------------
    # SUITE 1: Unique Needle in a Temp Text File
    # -------------------------------------------------------------
    needle_token = f"BENCH_KERNEL_NEEDLE_{uuid.uuid4().hex[:12]}"
    temp_file = os.path.join(LINUX_TREE, "temp_bench_needle_kernel.txt")
    print(f"\n[Suite 1] Unique Random Needle in Temp File: '{needle_token}'")
    with open(temp_file, "w") as f:
        f.write(f"/* Benchmark token line */\nconst char *needle = \"{needle_token}\";\n")

    try:
        # Part A: Time To First Match (TTFM)
        print("\n  --- Part A: Time To First Match (Interactive Discovery) ---")
        tools_ttfm = [
            ("ripgrep", ["rg", needle_token, LINUX_TREE]),
            ("grx", ["grx", needle_token, LINUX_TREE]),
            ("ugrep", ["ugrep", "-r", "--ignore-files", needle_token, LINUX_TREE]),
        ]
        for name, cmd in tools_ttfm:
            ttfm, match = run_ttfm(cmd, needle_token)
            if ttfm is not None:
                print(f"  {name:<14}: TTFM = {ttfm:6.2f} ms | {match[:60]}")
            else:
                print(f"  {name:<14}: Match not found")

        # Part B: Full Traversal (Scanning all 168k files to completion)
        print("\n  --- Part B: Full Traversal Across All 168k Files ---")
        print_table_header()
        tools_full = [
            ("ripgrep", f"rg '{needle_token}' '{LINUX_TREE}'"),
            ("grx", f"grx '{needle_token}' '{LINUX_TREE}'"),
            ("ugrep", f"ugrep -r --ignore-files '{needle_token}' '{LINUX_TREE}'"),
        ]
        for name, cmd in tools_full:
            res = run_bench(cmd, iterations=3)
            print_row(name, res)
    finally:
        if os.path.exists(temp_file):
            os.remove(temp_file)

    # -------------------------------------------------------------
    # SUITE 2: Literal Search on Common Symbol
    # -------------------------------------------------------------
    literal = "register_filesystem"
    print(f"\n[Suite 2] Literal Symbol Search: '{literal}' across entire kernel")
    print_table_header()
    tools_literal = [
        ("ripgrep", f"rg '{literal}' '{LINUX_TREE}'"),
        ("grx", f"grx '{literal}' '{LINUX_TREE}'"),
        ("ugrep", f"ugrep -r --ignore-files '{literal}' '{LINUX_TREE}'"),
    ]
    for name, cmd in tools_literal:
        res = run_bench(cmd, iterations=3)
        print_row(name, res)

    # -------------------------------------------------------------
    # SUITE 3: Complex Regex Pattern
    # -------------------------------------------------------------
    regex_pat = r"EXPORT_SYMBOL_GPL\([a-zA-Z0-9_]+\)"
    print(f"\n[Suite 3] Complex Regex Pattern: '{regex_pat}'")
    print_table_header()
    tools_regex = [
        ("ripgrep", f"rg -e '{regex_pat}' '{LINUX_TREE}'"),
        ("grx (re: DSL)", f"grx 're:{regex_pat}' '{LINUX_TREE}'"),
        ("grx (-E flag)", f"grx -E '{regex_pat}' '{LINUX_TREE}'"),
        ("ugrep", f"ugrep -r --ignore-files -e '{regex_pat}' '{LINUX_TREE}'"),
    ]
    for name, cmd in tools_regex:
        res = run_bench(cmd, iterations=3)
        print_row(name, res)

    # -------------------------------------------------------------
    # SUITE 4: Filetype Filtering (C files only)
    # -------------------------------------------------------------
    print("\n[Suite 4] Filetype Filtering on C Files: 'mutex_lock'")
    print_table_header()
    tools_ft = [
        ("ripgrep (-t c)", f"rg -t c 'mutex_lock' '{LINUX_TREE}'"),
        ("grx (:c DSL)", f"grx 'mutex_lock' :c '{LINUX_TREE}'"),
        ("ugrep (*.c)", f"ugrep -r --ignore-files --include='*.c' 'mutex_lock' '{LINUX_TREE}'"),
    ]
    for name, cmd in tools_ft:
        res = run_bench(cmd, iterations=3)
        print_row(name, res)

    print("\n" + "=" * 80)
    print("  BENCHMARK COMPLETE")
    print("=" * 80)

if __name__ == "__main__":
    main()
