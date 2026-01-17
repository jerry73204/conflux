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
# Build
# ==============================================================================

# Build core library only (pure Rust, no ROS2)
build-core:
    cargo build -p conflux-core

# Build ROS2 node with colcon
build:
    colcon build \
        --packages-select conflux \
        --symlink-install \
        --cmake-args -DCMAKE_BUILD_TYPE=RelWithDebInfo \
        --cargo-args --profile=test-release

# Build with verbose cargo output
build-verbose:
    colcon build \
        --packages-select conflux \
        --symlink-install \
        --cmake-args -DCMAKE_BUILD_TYPE=RelWithDebInfo \
        --cargo-args --profile=test-release -vv

# Build in debug mode (faster compilation)
build-debug:
    colcon build \
        --packages-select conflux \
        --symlink-install \
        --cmake-args -DCMAKE_BUILD_TYPE=Debug

# ==============================================================================
# Code Quality
# ==============================================================================

# Format code with nightly rustfmt
format:
    cargo +nightly fmt

# Run formatting check
format-check:
    cargo +nightly fmt --check

# Run clippy lints on core library
lint-core:
    cargo clippy -p conflux-core --all-targets -- -D warnings

# Run clippy lints on full workspace (requires prior build for ros2_cargo_config.toml)
lint:
    @if [ -f build/ros2_cargo_config.toml ]; then \
        cargo clippy --config build/ros2_cargo_config.toml --all-targets -- -D warnings; \
    else \
        echo "Note: build/ros2_cargo_config.toml not found. Run 'just build' first for full lint."; \
        cargo clippy -p conflux-core --all-targets -- -D warnings; \
    fi

# Run all checks (format + lint)
check: format-check lint

# ==============================================================================
# Testing
# ==============================================================================

# Run core library tests with cargo nextest
test-core:
    cargo nextest run -p conflux-core --features tokio --no-fail-fast

# Run core library tests with cargo test
test-core-cargo:
    cargo test -p conflux-core --features tokio

# Run full workspace tests with cargo nextest (requires prior build)
test:
    @if [ -f build/ros2_cargo_config.toml ]; then \
        cargo nextest run --config build/ros2_cargo_config.toml --cargo-profile test-release --no-fail-fast; \
    else \
        cargo nextest run -p conflux-core --features tokio --no-fail-fast; \
    fi

# Run tests with standard cargo test (if nextest not available)
test-cargo:
    @if [ -f build/ros2_cargo_config.toml ]; then \
        cargo test --config build/ros2_cargo_config.toml --profile test-release; \
    else \
        cargo test -p conflux-core --features tokio; \
    fi

# Alias for test-core (backwards compatibility)
test-lib: test-core

# ==============================================================================
# Running
# ==============================================================================

# Run the node (after build, requires .envrc sourced)
run:
    ros2 run conflux conflux-node

# ==============================================================================
# Maintenance
# ==============================================================================

# Clean all build artifacts
clean:
    rm -rf build install log target

# Clean only Rust artifacts
clean-rust:
    rm -rf target

# Clean only colcon artifacts
clean-colcon:
    rm -rf build install log
