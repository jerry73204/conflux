# CLAUDE.md

This file provides guidance to Claude Code when working with this repository.

## Environment Setup

**IMPORTANT: Ensure `.envrc` is sourced before working on this repository.**

This project uses [direnv](https://direnv.net/) to automatically source the ROS2 environment. The `.envrc` file:
- Sources `/opt/ros/humble/setup.bash` (ROS2 Humble)
- Sources `install/setup.bash` when available (built workspace)

Run `direnv allow` in the project directory if not already done. All commands in the justfile assume the environment is already set up.

## Project Overview

conflux is a multi-stream message synchronization library for ROS2. It groups messages from multiple topics that fall within configurable time windows.

### Packages

| Package | Language | Location | Description |
|---------|----------|----------|-------------|
| conflux-core | Rust | `crates/conflux-core/` | Pure Rust sync algorithm (no ROS2 deps) |
| conflux-ros2 | Rust | `crates/conflux-ros2/` | ROS2 utilities for Rust nodes |
| conflux | Rust | `conflux_node/` | Standalone ROS2 synchronization node |
| conflux_cpp | C++ | `conflux_cpp/` | C++ library wrapping core via FFI |
| conflux_py | Python | `conflux_py/` | Python library wrapping core via PyO3 |

## Build System

This project uses `colcon` to build ROS2 packages. The justfile provides all build commands.

**IMPORTANT: Always use the justfile for building and development tasks.**

```bash
just build          # Build all ROS2 packages (conflux, conflux_cpp, conflux_py)
just build-core     # Build only conflux-core (pure Rust, no ROS2)
just build-py       # Build Python wheel with maturin
```

## Common Development Commands

```bash
# Show all available commands
just

# ==== Setup ====
just setup              # Install colcon-cargo-ros2 extension

# ==== Building ====
just build              # Build all ROS2 packages with colcon
just build-pkg <name>   # Build specific package (e.g., just build-pkg conflux_cpp)
just build-verbose      # Build with verbose output
just build-debug        # Build in debug mode (faster compilation)
just build-core         # Build conflux-core only (pure Rust)
just build-py           # Build Python wheel with maturin

# ==== Testing ====
just test               # Run all tests (Rust, C++, Python)
just test-rust          # Run Rust tests (workspace + FFI + PyO3)
just test-core          # Run conflux-core tests only
just test-ffi           # Run conflux-ffi tests only
just test-py-rust       # Run conflux-py Rust tests only
just test-python        # Run Python tests with colcon

# ==== Formatting ====
just format             # Format all code (Rust, C++, Python)
just format-rust        # Format Rust code
just format-cpp         # Format C++ code
just format-python      # Format Python code
just format-check       # Check formatting without changes

# ==== Linting ====
just lint               # Run all lints (Rust, C++, Python)
just lint-rust          # Run clippy on all Rust code
just lint-cpp           # Run clang-tidy (requires prior build)
just lint-python        # Run ruff on Python code
just check              # Run format-check + lint

# ==== Running ====
just run                # Run the conflux node (after build)

# ==== Cleaning ====
just clean              # Clean all artifacts
just clean-rust         # Clean only Rust target/
just clean-colcon       # Clean only colcon build/install/log
```

## Project Structure

```
conflux/
├── Cargo.toml                    # Workspace root
├── justfile                      # Build commands
├── .envrc                        # ROS2 environment setup
│
├── crates/
│   ├── conflux-core/             # Core sync algorithm (pure Rust)
│   │   ├── src/
│   │   │   ├── lib.rs            # Public API, sync() function
│   │   │   ├── state.rs          # Core state machine
│   │   │   ├── buffer.rs         # Per-stream message buffering
│   │   │   ├── staleness.rs      # Message expiration system
│   │   │   └── types.rs          # WithTimestamp trait, Key trait
│   │   └── tests/                # Integration tests
│   │
│   └── conflux-ros2/             # ROS2 utilities (Rust)
│       └── src/
│           ├── lib.rs            # Public API
│           ├── message.rs        # TimestampedMessage wrapper
│           └── subscriber.rs     # Dynamic subscription utilities
│
├── conflux_node/                 # Standalone ROS2 node
│   ├── package.xml               # ROS2 package manifest (name: conflux)
│   ├── config/                   # YAML configuration files
│   ├── launch/                   # ROS2 launch files
│   └── src/
│
├── conflux_cpp/                  # C++ ROS2 library
│   ├── package.xml
│   ├── CMakeLists.txt
│   ├── include/conflux/          # Public headers
│   │   ├── synchronizer.hpp      # Main C++ API
│   │   └── types.hpp             # SyncGroup, Config types
│   ├── src/                      # Implementation
│   ├── examples/sync_node.cpp    # Example ROS2 node
│   └── rust/                     # FFI crate (built by CMake)
│
├── conflux_py/                   # Python ROS2 library
│   ├── package.xml
│   ├── pyproject.toml            # maturin build config
│   ├── conflux_py/
│   │   ├── __init__.py           # Exports Synchronizer, SyncConfig, SyncGroup
│   │   ├── synchronizer.py       # ROS2Synchronizer wrapper
│   │   └── _conflux_py.pyi       # Type stubs
│   ├── examples/sync_node.py     # Example ROS2 node
│   ├── test/                     # pytest tests
│   └── rust/                     # PyO3 crate (built by maturin)
│
├── test/                         # Launch file test scripts
└── docs/roadmap/                 # Development documentation
```

## Usage Examples

### conflux-core (Pure Rust)

```rust
use conflux_core::{sync, Config, WithTimestamp};
use std::time::Duration;

// Implement WithTimestamp for your message type
impl WithTimestamp for MyMessage {
    fn timestamp(&self) -> Duration { self.ts }
}

// Synchronize streams
let config = Config::basic(Duration::from_millis(50), None, 64);
let (output_stream, feedback) = sync(input_stream, ["camera", "lidar"], config)?;
```

### conflux_cpp (C++ ROS2)

```cpp
#include "conflux/synchronizer.hpp"
#include <sensor_msgs/msg/image.hpp>

class MyNode : public rclcpp::Node {
    std::unique_ptr<conflux::Synchronizer> sync_;

    MyNode() : Node("my_node") {
        conflux::Config config;
        config.window_size = std::chrono::milliseconds(50);
        sync_ = std::make_unique<conflux::Synchronizer>(config);

        sync_->add_subscription<sensor_msgs::msg::Image>(shared_from_this(), "/camera/image");
        sync_->on_synchronized([this](const conflux::SyncGroup& group) {
            auto image = group.get<sensor_msgs::msg::Image>("/camera/image");
            // Process synchronized messages
        });
    }
};
```

### conflux_py (Python ROS2)

```python
from conflux_py import ROS2Synchronizer
from sensor_msgs.msg import Image, PointCloud2

class MyNode(Node):
    def __init__(self):
        super().__init__("my_node")
        sync = ROS2Synchronizer(self, window_size_ms=50)
        sync.add_subscription(Image, "/camera/image")
        sync.add_subscription(PointCloud2, "/lidar/points")

        @sync.on_synchronized
        def callback(group):
            image = group["/camera/image"]
            points = group["/lidar/points"]
            # Process synchronized messages
```

### Low-level Python API

```python
from conflux_py import Synchronizer, SyncConfig

config = SyncConfig(window_size_ms=50, buffer_size=64)
sync = Synchronizer(["/camera", "/lidar"], config)

sync.push("/camera", timestamp_ns, camera_msg)
sync.push("/lidar", timestamp_ns, lidar_msg)

for group in sync:
    camera = group["/camera"]
    lidar = group["/lidar"]
```

## Configuration (conflux_node)

The standalone node requires a YAML configuration file:

```yaml
inputs:
  - topic: /camera/image
    type: sensor_msgs/msg/Image
  - topic: /lidar/points
    type: sensor_msgs/msg/PointCloud2

output:
  topic: /synced

sync:
  window_size: 50ms
  buffer_size: 64
```

See `conflux_node/config/example.yaml` for full documentation.

## Build Notes

- **Workspace crates** (`conflux-core`): Can be built with `cargo build -p conflux-core`
- **ROS2 crates** (`conflux-ros2`, `conflux_node`): Must be built with `just build` (colcon)
- **FFI crate** (`conflux_cpp/rust`): Built automatically by CMake during `just build`
- **PyO3 crate** (`conflux_py/rust`): Built with `just build-py` (maturin) or during colcon build

ROS2 message crates use wildcard versions (`*`) that are patched by `build/ros2_cargo_config.toml` at build time.

## Testing

```bash
# Core library tests (no ROS2 required)
just test-core          # 166 tests

# All Rust tests (core + FFI + PyO3)
just test-rust

# Python tests (requires built wheel)
cd /tmp && python -m pytest /path/to/conflux_py/test/

# Launch file tests
bash test/test_all_launches.sh
```

## Launch Files

```bash
# Basic launch
ros2 launch conflux conflux.launch.xml config:=/path/to/config.yaml

# Preset configurations
ros2 launch conflux realtime_sync.launch.xml      # Camera + LiDAR
ros2 launch conflux multi_sensor_fusion.launch.xml # Camera + LiDAR + IMU
ros2 launch conflux stereo_camera.launch.xml       # Stereo pair (5ms window)
```
