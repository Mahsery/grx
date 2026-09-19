.PHONY: all build install test clean bump bump-minor bump-major release tutorial bench help

CARGO ?= cargo
PYTHON ?= python3

all: build

build:
	$(CARGO) build --release

install:
	$(CARGO) install --path . --force

test:
	$(CARGO) test

clean:
	$(CARGO) clean

bump:
	$(PYTHON) scripts/bump_version.py

bump-patch:
	$(PYTHON) scripts/bump_version.py --patch

bump-minor:
	$(PYTHON) scripts/bump_version.py --minor

bump-major:
	$(PYTHON) scripts/bump_version.py --major

tutorial:
	$(CARGO) run --release -- --tutorial

bench:
	@bash scripts/bench_hyperfine.sh $(LINUX_TREE)

release: bump build test install
	@echo "grx: release build, test, and install completed successfully."

help:
	@echo "grx Makefile targets:"
	@echo "  build        - Build optimized release binary"
	@echo "  install      - Install binary into ~/.cargo/bin/grx"
	@echo "  test         - Run test suite"
	@echo "  bench        - Run comparative hyperfine benchmark suite"
	@echo "  bump         - Bump patch version in Cargo.toml (e.g. 0.1.0 -> 0.1.1)"
	@echo "  bump-minor   - Bump minor version in Cargo.toml (e.g. 0.1.0 -> 0.2.0)"
	@echo "  bump-major   - Bump major version in Cargo.toml (e.g. 0.1.0 -> 1.0.0)"
	@echo "  tutorial     - Run interactive grx search DSL tutorial"
	@echo "  release      - Bump patch, build release, test, and install"
	@echo "  clean        - Clean target directory"
