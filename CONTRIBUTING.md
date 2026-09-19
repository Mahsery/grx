# Contributing to `grx`

Thank you for your interest in contributing to `grx`! Contributions, bug reports, and suggestions are welcome.

## Development Setup

To get started, ensure you have a standard Rust toolchain installed:

```bash
git clone https://github.com/Mahsery/grx.git
cd grx
cargo build
```

## Running Tests & Checks

Before opening a pull request, make sure tests, lints, and formatting pass:

```bash
# Run test suite
cargo test

# Check code formatting
cargo fmt --all -- --check

# Run compiler and clippy lints
cargo clippy --all-targets -- -D warnings
```

## Pull Request Guidelines

1. **Atomic Commits:** Keep commits focused on a single logical change or fix.
2. **Commit Messages:** Use clear, concise commit messages explaining *what* was changed and *why*.
3. **Tests:** Include tests for any bug fixes or new query filters.
4. **Clean CI:** Verify `cargo test` and `cargo clippy` pass cleanly.

## Benchmarks

Comparative benchmarks can be run with:

```bash
cargo build --release
python3 scripts/bench_comparison.py
```

## License

By contributing to `grx`, you agree that your contributions will be licensed under the dual MIT / Apache-2.0 licenses.
