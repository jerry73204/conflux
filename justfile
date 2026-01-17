# conflux - Multi-stream synchronization ROS2 node
# Built with colcon-cargo-ros2

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

# Build with colcon (ROS2 build system)
build:
    colcon build \
        --symlink-install \
        --cmake-args -DCMAKE_BUILD_TYPE=RelWithDebInfo \
        --cargo-args --profile=test-release

# Build with verbose cargo output
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
# Code Quality
# ==============================================================================

# Format code with nightly rustfmt
format:
    cargo +nightly fmt

# Run formatting check
format-check:
    cargo +nightly fmt --check

# Run clippy lints (requires prior build to generate ros2_cargo_config.toml)
lint:
    @if [ -f build/ros2_cargo_config.toml ]; then \
        cargo clippy --config build/ros2_cargo_config.toml --all-targets -- -D warnings; \
    else \
        echo "Note: build/ros2_cargo_config.toml not found. Run 'just build' first for full lint."; \
        cargo clippy --all-targets -- -D warnings; \
    fi

# Run all checks (format + lint)
check: format-check lint

# ==============================================================================
# Testing
# ==============================================================================

# Run tests with cargo nextest
test:
    @if [ -f build/ros2_cargo_config.toml ]; then \
        cargo nextest run --config build/ros2_cargo_config.toml --cargo-profile test-release --no-fail-fast; \
    else \
        cargo nextest run --cargo-profile test-release --no-fail-fast; \
    fi

# Run tests with standard cargo test (if nextest not available)
test-cargo:
    @if [ -f build/ros2_cargo_config.toml ]; then \
        cargo test --config build/ros2_cargo_config.toml --profile test-release; \
    else \
        cargo test --profile test-release; \
    fi

# Run multi-stream-synchronizer library tests
test-lib:
    cargo test -p multi-stream-synchronizer --features tokio

# ==============================================================================
# Running
# ==============================================================================

# Run the node (after build, requires .envrc sourced)
run:
    ros2 run conflux conflux

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

# Update git submodules
submodules:
    git submodule update --init --recursive
