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

# Run Rust tests with nextest
test-rust-nextest:
    cargo nextest run --workspace --no-fail-fast
    cd conflux_cpp/rust && cargo nextest run --no-fail-fast

# Run C++ tests (currently no unit tests, only lint checks available)
test-cpp:
    @echo "C++ unit tests: none defined yet"
    @echo "Run 'just colcon-test' for ament_lint style checks"

# Run Python tests
test-python:
    @if [ -d "conflux_py" ]; then \
        colcon test --packages-select conflux_py; \
        colcon test-result --verbose; \
    else \
        echo "No Python tests (conflux_py not yet created)"; \
    fi

# Run conflux-core tests only
test-core:
    cargo test -p conflux-core --features tokio

# Run conflux-core tests with nextest
test-core-nextest:
    cargo nextest run -p conflux-core --features tokio --no-fail-fast

# Run conflux-ffi tests only
test-ffi:
    cd conflux_cpp/rust && cargo test

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
        grep -v '/target/' | \
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

# Run all checks (format-check + lint)
check: format-check lint-rust

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
