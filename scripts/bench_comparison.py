#!/usr/bin/env python3
"""Benchmark grx against ripgrep and an optional baseline."""

import argparse
from dataclasses import dataclass
import hashlib
from pathlib import Path
import json
import shlex
import shutil
import subprocess
import tempfile

REPO_DIR = Path(__file__).resolve().parent.parent
CONTENT_FLAGS = ("--color=never", "--no-heading", "-N", "-s")
GRX_SELECTION_FLAGS = ("--hidden", "--no-ignore")
RG_SELECTION_FLAGS = ("--hidden", "--no-ignore")
ABSENT = "__GRX_BENCH_ABSENT_20261008__"
NEEDLE = "TARGET_FOUND_HERE"

@dataclass(frozen=True)
class Case:
    name: str
    grx: tuple[str, ...]
    reference: tuple[str, ...]
    cwd: Path
    expected_status: int = 0
    expected_records: tuple[bytes, ...] | None = None
    note: str = ""

def canonical_records(stdout):
    """Preserve full records and repeated occurrences, ignoring display order."""
    if not stdout:
        return ()
    records = stdout.split(b"\n")
    if stdout.endswith(b"\n"):
        records.pop()

    return tuple(sorted(line.removeprefix(b"./") for line in records))

def run_checked(command, cwd, expected_status, timeout):
    result = subprocess.run(
        command, cwd=cwd, stdin=subprocess.DEVNULL, capture_output=True,
        timeout=timeout, check=False,
    )
    if result.returncode != expected_status or result.stderr:
        raise RuntimeError(
            f"{shlex.join(command)}: expected status {expected_status}, "
            f"got {result.returncode}; stderr={result.stderr.decode(errors='replace')!r}"
        )
    return canonical_records(result.stdout)

def require_records(actual, expected, label):
    if actual != expected:
        first = next(
            ((a, b) for a, b in zip(actual, expected) if a != b),
            (actual[len(expected):len(expected) + 1], expected[len(actual):len(actual) + 1]),
        )
        raise RuntimeError(
            f"{label}: full-record mismatch ({len(actual)} vs {len(expected)} records); "
            f"first difference: {first!r}"
        )

def verify_case(case, timeout, baseline_grx=None):
    commands = (case.grx, case.reference)
    labels = ("grx", "reference")
    if baseline_grx is not None:
        commands = (case.grx, (baseline_grx, *case.grx[1:]), case.reference)
        labels = ("candidate", "baseline", "reference")
    records = [run_checked(command, case.cwd, case.expected_status, timeout)
               for command in commands]
    expected = records[-1]
    for label, actual in zip(labels, records):
        require_records(actual, expected, f"{case.name} {label}")
    if case.expected_records is not None:
        require_records(expected, case.expected_records, f"{case.name} fixture oracle")
    print(f"[{case.name}] verified {len(expected)} complete records; status={case.expected_status}", flush=True)
    return commands, labels, expected

def run_case(case, verified, hyperfine, min_runs, warmup, timeout):
    commands, labels, expected = verified
    invocation = [hyperfine, "--shell=none", "--input=null", "--output=pipe",
                  "--metrics=all", "--warmup", str(warmup),
                  "--min-runs", str(min_runs), "--style=none",
                  "--export-json", "/dev/stdout"]
    if case.expected_status != 0:

        invocation.append(f"--ignore-failure={case.expected_status}")
    for label, command in zip(labels, commands):
        invocation.extend(("--command-name", label, shlex.join(command)))
    print(f"\n[{case.name}] Hyperfine: warmup={warmup}, min-runs={min_runs}, adaptive sampling", flush=True)
    for label, command in zip(labels, commands):
        print(f"  {label}: {shlex.join(command)}", flush=True)
    if case.note:
        print(f"  {case.note}", flush=True)

    completed = subprocess.run(invocation, cwd=case.cwd, stdin=subprocess.DEVNULL,
                               stdout=subprocess.PIPE, check=True)

    decoder = json.JSONDecoder()
    stream = completed.stdout.decode()
    offset = 0
    report = None
    while offset < len(stream):
        while offset < len(stream) and stream[offset].isspace():
            offset += 1
        if offset == len(stream):
            break
        report, offset = decoder.raw_decode(stream, offset)
    if report is None:
        raise RuntimeError(f"{case.name}: Hyperfine produced no report")
    if report.get("schema_version") != 2:
        raise RuntimeError("Hyperfine 2.x JSON schema is required")
    results = report["results"]
    if len(results) != len(commands):
        raise RuntimeError(f"{case.name}: missing Hyperfine results")
    for label, result, command in zip(labels, results, commands):
        samples = result["measurements"]
        if len(samples) < min_runs:
            raise RuntimeError(f"{case.name} {label}: insufficient Hyperfine samples")
        for sample in samples:
            if sample["exit_code"] != case.expected_status:
                raise RuntimeError(f"{case.name} {label}: unexpected measured exit status {sample['exit_code']}")
        require_records(run_checked(command, case.cwd, case.expected_status, timeout),
                        expected, f"{case.name} {label} after Hyperfine")

        print(json.dumps({"case": case.name, "engine": label,
                          "hyperfine_version": report["metadata"]["hyperfine_version"],
                          "summary": result["summary"]}, sort_keys=True), flush=True)

def executable_path(value):
    path = shutil.which(value)
    if path is None:
        raise argparse.ArgumentTypeError(f"executable not found: {value}")
    return str(Path(path).resolve())

def print_identity(label, executable):
    result = subprocess.run([executable, "--version"], capture_output=True, check=True)
    version = result.stdout.decode(errors="replace").splitlines()[0]
    digest = hashlib.sha256(Path(executable).read_bytes()).hexdigest()
    print(f"{label}: {executable}\n  {version}\n  sha256={digest}", flush=True)
    return digest

def content_command(executable, args, *, grx, jobs=None):
    flags = GRX_SELECTION_FLAGS if grx else RG_SELECTION_FLAGS
    thread_args = () if jobs is None else ("--threads", str(jobs))
    return (executable, *CONTENT_FLAGS, *flags, *thread_args, *args)

def discovery_command(executable, args, *, grx):
    if grx:
        return (executable, "--color=never", "-s", *GRX_SELECTION_FLAGS, *args)
    return (executable, "--files", *RG_SELECTION_FLAGS, *args)

def source_cases(grx, rg, source):
    root = f"root:{source}"
    def case(name, left, right, status=0, expected=None):
        return Case(name, content_command(grx, (*left, root), grx=True),
                    content_command(rg, (*right, str(source)), grx=False),
                    REPO_DIR, status, expected)
    return [
        case("source_literal", ("in:AdaptiveReader",), ("AdaptiveReader",)),
        case("source_boolean", ("in:pub", "AND", "in:fn"), ("pub.*fn|fn.*pub",)),
        case("source_typed", ("t:rs", "in:pub"), ("-t", "rust", "pub")),
        case("source_no_match", (f"in:{ABSENT}",), (ABSENT,), 1, ()),
    ]

def create_corpus(root):
    """Create 1,000 files with 100 marker lines."""
    sample = b"const DEFAULT_TIMEOUT_SECS: u64 = 30;\n"
    needle = b'const SPECIAL_TOKEN_FOR_BENCHMARK: &str = "TARGET_FOUND_HERE";\n'
    records = []
    for index in range(1000):
        relative = Path("codex" if index % 2 == 0 else "codexish") / f"file_{index:04d}.rs"
        path = root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        marked = index % 10 == 0
        body = sample * 600 + (needle if marked else sample) + sample * 599
        path.write_bytes(body)
        if marked:
            records.append(relative.as_posix().encode() + b":" + needle.rstrip(b"\n"))
    for relative, body in (
        ("codex/memory.rs", b"name-only unrelated\n"),
        ("codex/nested/memory_summary.md", b"name-only unrelated\n"),
        ("codex/body_only.rs", b"memory\nmxxmxxrxxy\nmy_rem\n"),
        ("codex/mxxmxxrxxy.rs", b"name-only unrelated\n"),
        ("codexish/memory.rs", b"name-only unrelated\n"),
        ("nested/codex/memory.py", b"memory\n"),
    ):
        path = root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(body)
    files = [path for path in root.rglob("*") if path.is_file()]
    size = sum(path.stat().st_size for path in files)
    print(f"\nSynthetic corpus: {len(files)} files, {size} bytes ({size / 2**20:.2f} MiB); 100 marker lines", flush=True)
    return tuple(sorted(records))

def synthetic_cases(grx, rg, root, marker_records):
    def content(name, left, right, *, jobs=None, status=0, expected=None, note=""):
        return Case(
            name, content_command(grx, (*left, "root:."), grx=True, jobs=jobs),
            content_command(rg, (*right, "."), grx=False, jobs=jobs),
            root, status, expected, note,
        )
    def names(name, left, glob, expected=None, note=""):
        return Case(name, discovery_command(grx, (*left, "root:."), grx=True),
                    discovery_command(rg, ("-g", glob, "."), grx=False),
                    root, 0, expected, note)
    memory_names = tuple(sorted(path.encode() for path in (
        "codex/memory.rs", "codex/nested/memory_summary.md",
        "codexish/memory.rs", "nested/codex/memory.py",
    )))
    fuzzy_names = tuple(sorted((*memory_names, b"codex/mxxmxxrxxy.rs")))
    scoped_names = tuple(path for path in memory_names if not path.startswith(b"codexish/"))
    return [
        content("synthetic_content_j1", (f"in:{NEEDLE}",), (NEEDLE,),
                jobs=1, expected=marker_records),
        content("synthetic_content_j4", (f"in:{NEEDLE}",), (NEEDLE,),
                jobs=4, expected=marker_records),
        content("synthetic_typed", ("t:rs", f"in:{NEEDLE}"), ("-t", "rust", NEEDLE),
                expected=marker_records),
        content("synthetic_path", ("p:codex/", f"in:{NEEDLE}"),
                ("-g", "**/codex/**", NEEDLE), expected=marker_records),
        content("synthetic_no_match", (f"in:{ABSENT}",), (ABSENT,), status=1, expected=()),
        names("discovery_substring", ("kind:file", "memory"), "*memory*", memory_names),
        names("discovery_exact", ("kind:file", "=memory.rs"), "memory.rs",
              tuple(sorted((b"codex/memory.rs", b"codexish/memory.rs")))),
        names("discovery_path", ("kind:file", "p:codex/", "memory"),
              "**/codex/**/*memory*", scoped_names),
        names("discovery_type", ("t:rs",), "*.rs"),
        names("fuzzy_names", ("kind:file", "fz:mmry"), "*m*m*r*y*", fuzzy_names,
              "Reference verifies ordered-subsequence membership; it does not score or rank."),
        content("fuzzy_content", ("in:fz:TFH",), ("T.*F.*H",),
                expected=marker_records,
                note="Reference verifies ordered-subsequence membership; it does not score or rank."),
    ]

def warmup_count(value):
    number = int(value)
    if number < 1:
        raise argparse.ArgumentTypeError("at least one warmup is required")
    return number

def sample_count(value):
    number = int(value)
    if number < 10:
        raise argparse.ArgumentTypeError("at least 10 measured runs are required")
    return number

def main():
    parser = argparse.ArgumentParser(description=__doc__, allow_abbrev=False)
    parser.add_argument("--grx", type=executable_path, default=str(REPO_DIR / "target/release/grx"))
    parser.add_argument("--baseline-grx", type=executable_path,
                        help="Preserved baseline executable; adds the same full matrix")
    parser.add_argument("--rg", type=executable_path, default="rg")
    parser.add_argument("--warmup", type=warmup_count, default=5)
    parser.add_argument("--min-runs", type=sample_count, default=20,
                        help="Hyperfine adaptive sampling minimum; no fixed/max run count")
    parser.add_argument("--verification-timeout", type=int, default=300)
    args = parser.parse_args()
    try:
        hyperfine = executable_path("hyperfine")
    except argparse.ArgumentTypeError as error:
        parser.error(str(error))
    version = subprocess.run([hyperfine, "--version"], capture_output=True, check=True).stdout.decode().strip()
    if not version.startswith("hyperfine 2."):
        parser.error("Hyperfine 2.x is required for measured status and CPU/counter metrics")
    if args.verification_timeout < 1:
        parser.error("verification timeout must be positive")
    print(f"Timing engine: {version}; full mandatory matrix; no Python timing", flush=True)
    candidate_digest = print_identity("candidate" if args.baseline_grx else "grx", args.grx)
    if args.baseline_grx:
        baseline_digest = print_identity("baseline", args.baseline_grx)
        if candidate_digest == baseline_digest:
            print("CALIBRATION: identical binaries; this cannot establish old/new performance.", flush=True)
    print_identity("reference", args.rg)
    source = REPO_DIR / "src"
    files = [path for path in source.rglob("*") if path.is_file()]
    print(f"Source corpus: {len(files)} files, {sum(path.stat().st_size for path in files)} bytes", flush=True)

    with tempfile.TemporaryDirectory(prefix="grx-bench-") as directory:
        root = Path(directory)
        cases = source_cases(args.grx, args.rg, source)
        cases.extend(synthetic_cases(args.grx, args.rg, root, create_corpus(root)))

        verified = [verify_case(case, args.verification_timeout, args.baseline_grx)
                    for case in cases]
        for case, preflight in zip(cases, verified):
            run_case(case, preflight, hyperfine, args.min_runs, args.warmup,
                     args.verification_timeout)
    print(f"\nBENCHMARK SUITE COMPLETE: all {len(cases)} cases timed by Hyperfine; "
          "full records verified before/after and every measured exit status checked.", flush=True)

if __name__ == "__main__":
    main()
