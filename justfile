# conflux - Multi-stream synchronization workspace
# Built with colcon-cargo-ros2 for ROS2 integration

# Show available commands
default:
    @just --list

# ==============================================================================
# Setup
# ==============================================================================

# Install colcon-cargo-ros2 extension
setup:
    pip install -U colcon-cargo-ros2

# ==============================================================================
# Build - Colcon (ROS2 packages)
# ==============================================================================

# Build all ROS2 packages with colcon
build:
    colcon build \
        --symlink-install \
        --cmake-args -DCMAKE_BUILD_TYPE=RelWithDebInfo -DCMAKE_EXPORT_COMPILE_COMMANDS=ON \
        --cargo-args --profile=test-release

# Build specific ROS2 package
build-pkg pkg:
    colcon build \
        --packages-select {{pkg}} \
        --symlink-install \
        --cmake-args -DCMAKE_BUILD_TYPE=RelWithDebInfo \
        --cargo-args --profile=test-release

# Build with verbose output
build-verbose:
    colcon build \
        --symlink-install \
        --cmake-args -DCMAKE_BUILD_TYPE=RelWithDebInfo \
        --cargo-args --profile=test-release -vv

# Build in debug mode (faster compilation)
build-debug:
    colcon build \
        --symlink-install \
        --cmake-args -DCMAKE_BUILD_TYPE=Debug

# ==============================================================================
# Build - Cargo (Rust crates only)
# ==============================================================================

# Build all Rust crates (workspace members only, no ROS2 deps)
cargo-build:
    cargo build --workspace

# Build Rust crates in release mode
cargo-build-release:
    cargo build --workspace --release

# Build conflux-core only
cargo-build-core:
    cargo build -p conflux-core

# Build conflux-ffi crate (for C++ bindings)
cargo-build-ffi:
    cd conflux_cpp/rust && cargo build --release

# Build conflux-ffi and copy library (Python uses FFI via ctypes)
cargo-build-py: cargo-build-ffi

# ==============================================================================
# Testing
# ==============================================================================

# Run all tests (Rust, C++, Python)
test: test-rust test-cpp test-python

# Run Rust tests
test-rust:
    cargo test --workspace
    cd conflux_cpp/rust && cargo test
    just test-ros2

# Run Rust tests with nextest
test-rust-nextest:
    cargo nextest run --workspace --no-fail-fast
    cd conflux_cpp/rust && cargo nextest run --no-fail-fast

# Run C++ tests (gtest + ament lint)
# L-22: this recipe used to echo two lines and exit 0, so `just test` reported a
# passing C++ suite that did not exist.
#
# L-27: it is no longer scoped to the gtest target. The ament linters
# (copyright, cppcheck, cpplint, lint_cmake, xmllint) are green, so they gate
# too. `ament_uncrustify` is deliberately not among them -- this project formats
# C++ with clang-format and the two styles are incompatible; see package.xml.
test-cpp:
    colcon test --packages-select conflux_cpp --event-handlers console_direct+ \
        --return-code-on-test-failure

# Run Python tests
# NOTE: pytest is invoked directly rather than via `colcon test`. colcon runs
# `setup.py test` (unittest) for ament_python packages, which collects 0 of these
# pytest-style tests and still exits 0 -- silently reporting success.
test-python:
    @if [ -d "conflux_py/test" ]; then \
        python3 -m pytest conflux_py/test/ -v; \
    else \
        echo "No Python tests (conflux_py not yet created)"; \
    fi

# Run conflux-core tests only
test-core:
    cargo test -p conflux-core

# Run conflux-core tests with nextest
test-core-nextest:
    cargo nextest run -p conflux-core --no-fail-fast

# Run conflux-ffi tests only
test-ffi:
    cd conflux_cpp/rust && cargo test

# Run conflux-ros2 tests
#
# H-14: this crate is excluded from the cargo workspace (its ROS message deps are
# wildcards patched by colcon at build time), so nothing ran its tests -- which is
# how a whole duplicate synchronization algorithm sat in it, covered only by tests
# that exercised a bare VecDeque. It needs the colcon-generated patch config, so
# `just build` must have run at least once.
test-ros2:
    @if [ -f build/ros2_cargo_config.toml ]; then \
        cd crates/conflux-ros2 && \
        cargo test --config "$PWD/../../build/ros2_cargo_config.toml"; \
    else \
        echo "build/ros2_cargo_config.toml missing -- run 'just build' first"; \
        exit 1; \
    fi

# ==============================================================================
# Test - Colcon (ROS2 packages)
# ==============================================================================

# Run colcon tests for all packages
colcon-test:
    colcon test

# Run colcon tests with verbose output
colcon-test-verbose:
    colcon test --event-handlers console_direct+

# Show colcon test results
colcon-test-result:
    colcon test-result --verbose

# ==============================================================================
# Formatting
# ==============================================================================

# Format all code (Rust, C++, Python)
format: format-rust format-cpp format-python

# Format Rust code
format-rust:
    cargo +nightly fmt --all
    cd conflux_cpp/rust && cargo +nightly fmt

# Format C++ code
format-cpp:
    find conflux_cpp -name '*.cpp' -o -name '*.hpp' -o -name '*.h' | \
        grep -v '/target/' | \
        xargs -r clang-format -i

# Format Python code
format-python:
    @if [ -d "conflux_py" ]; then \
        ruff format conflux_py || black conflux_py 2>/dev/null || echo "No Python formatter found (install ruff or black)"; \
    else \
        echo "No Python code to format (conflux_py not yet created)"; \
    fi

# ==============================================================================
# Format Checking
# ==============================================================================

# Check all formatting (Rust, C++, Python)
format-check: format-check-rust format-check-cpp format-check-python

# Check Rust formatting
format-check-rust:
    cargo +nightly fmt --all --check
    cd conflux_cpp/rust && cargo +nightly fmt --check

# Check C++ formatting
format-check-cpp:
    @find conflux_cpp -name '*.cpp' -o -name '*.hpp' -o -name '*.h' | \
        grep -v '/target/' | grep -v 'conflux/conflux_ffi.h' | \
        xargs -r clang-format --dry-run --Werror

# Check Python formatting
format-check-python:
    @if [ -d "conflux_py" ]; then \
        ruff format --check conflux_py || black --check conflux_py 2>/dev/null || echo "No Python formatter found"; \
    else \
        echo "No Python code to check (conflux_py not yet created)"; \
    fi

# ==============================================================================
# Linting
# ==============================================================================

# Run all lints
lint: lint-rust lint-cpp lint-python

# Lint Rust code
lint-rust:
    cargo clippy --workspace --all-targets -- -D warnings
    cd conflux_cpp/rust && cargo clippy --all-targets -- -D warnings

# Lint C++ code (requires clang-tidy and prior build)
lint-cpp:
    @if [ -f "build/conflux_cpp/compile_commands.json" ]; then \
        find conflux_cpp/src conflux_cpp/include -name '*.cpp' -o -name '*.hpp' | \
            grep -v '/target/' | \
            xargs clang-tidy -p build/conflux_cpp --warnings-as-errors='*'; \
    else \
        echo "compile_commands.json not found. Run 'just build' first."; \
        exit 1; \
    fi

# Lint Python code
lint-python:
    @if [ -d "conflux_py" ]; then \
        ruff check conflux_py || echo "ruff not found, skipping Python lint"; \
    else \
        echo "No Python code to lint (conflux_py not yet created)"; \
    fi

# Run all checks (format-check + lint), continues even if some fail
check:
    #!/usr/bin/env bash
    set -o pipefail
    failed=0
    echo "=== Format Check: Rust ==="
    just format-check-rust || failed=1
    echo ""
    echo "=== Format Check: C++ ==="
    just format-check-cpp || failed=1
    echo ""
    echo "=== Format Check: Python ==="
    just format-check-python || failed=1
    echo ""
    echo "=== Lint: Rust ==="
    just lint-rust || failed=1
    echo ""
    echo "=== Lint: C++ ==="
    just lint-cpp || failed=1
    echo ""
    echo "=== Lint: Python ==="
    just lint-python || failed=1
    echo ""
    if [ $failed -ne 0 ]; then
        echo "❌ Some checks failed"
        exit 1
    else
        echo "✅ All checks passed"
    fi

# ==============================================================================
# Running
# ==============================================================================

# Run the conflux node (after build, requires .envrc sourced)
run:
    ros2 run conflux conflux-node

# ==============================================================================
# Maintenance
# ==============================================================================

# Clean all build artifacts
clean: clean-colcon clean-rust

# Clean only Rust artifacts
clean-rust:
    rm -rf target
    rm -rf conflux_cpp/rust/target

# Clean only colcon artifacts
clean-colcon:
    rm -rf build install log

# ==============================================================================
# Aliases (backwards compatibility)
# ==============================================================================

# Alias: build core library
build-core: cargo-build-core

# Alias: cargo-test
cargo-test: test-rust

# Alias: cargo-test-nextest
cargo-test-nextest: test-rust-nextest

# Alias: cargo-test-core
cargo-test-core: test-core

# Alias: cargo-test-ffi
cargo-test-ffi: test-ffi
