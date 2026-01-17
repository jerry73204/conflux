# CLAUDE.md

This file provides guidance to Claude Code when working with this repository.

## Environment Setup

**IMPORTANT: Ensure `.envrc` is sourced before working on this repository.**

This project uses [direnv](https://direnv.net/) to automatically source the ROS2 environment. The `.envrc` file:
- Sources `/opt/ros/humble/setup.bash` (ROS2 Humble)
- Sources `install/setup.bash` when available (built workspace)

Run `direnv allow` in the project directory if not already done. All commands in the justfile assume the environment is already set up.

## Project Overview

conflux is a **Cargo workspace** for multi-stream message synchronization. It consists of:

- **conflux-core** (`crates/conflux-core/`): Pure Rust library implementing the time-window based synchronization algorithm. No ROS2 dependencies - can be used in any Rust project.
- **conflux-ros2** (`crates/conflux-ros2/`): ROS2 integration utilities - dynamic subscriptions, timestamp extraction, message wrappers. Enables embedding sync in your own ROS2 nodes.
- **conflux_node** (`conflux_node/`): Standalone ROS2 node that uses conflux-ros2 to synchronize messages from multiple input topics.
- **conflux_cpp** (`conflux_cpp/`): C++ ROS2 library wrapping conflux-core via FFI. For C++ ROS2 developers.
- **conflux_py** (`conflux_py/`): Python ROS2 library wrapping conflux-core via PyO3. For Python ROS2 developers.

This workspace structure enables:
- Using the core algorithm as a standalone Rust library
- Embedding synchronization in custom ROS2 nodes via conflux-ros2 (Rust)
- Using the synchronizer in C++ ROS2 nodes via conflux_cpp
- Using the synchronizer in Python ROS2 nodes via conflux_py

## Build System

This project uses `colcon-cargo-ros2` to build Rust code as a ROS2 package.

**IMPORTANT: Always use the justfile for building and development tasks.**

```bash
# Build the core library only (no ROS2 dependencies)
just build-core

# Build the ROS2 node (ALWAYS use this for full build)
just build

# Never run colcon or cargo commands directly for building the ROS2 node
# The justfile handles proper ROS2 environment setup and configuration
```

## Common Development Commands

```bash
# Show available commands
just

# Install colcon-cargo-ros2 extension
just setup

# Build core library only (pure Rust)
just build-core

# Build ROS2 node with colcon (primary build method)
just build

# Build with verbose output
just build-verbose

# Build in debug mode (faster compilation)
just build-debug

# Format code
just format

# Check formatting without changes
just format-check

# Run lints on core library
just lint-core

# Run lints on full workspace (requires prior build)
just lint

# Run all checks (format-check + lint)
just check

# Run core library tests
just test-core

# Run full workspace tests (requires prior build)
just test

# Run tests with cargo test
just test-cargo

# Alias for test-core
just test-lib

# Run the node (after build)
just run

# Clean all artifacts
just clean

# Clean only Rust target/
just clean-rust

# Clean only colcon build/install/log
just clean-colcon
```

## Project Structure

```
conflux/
├── Cargo.toml                    # Workspace root
├── README.md                     # Project documentation
├── LICENSE-MIT                   # MIT license
├── LICENSE-APACHE                # Apache 2.0 license
├── justfile                      # Build and development commands
├── .envrc                        # direnv config (sources ROS2)
│
├── crates/
│   ├── conflux-core/             # Core sync algorithm (pure Rust)
│   │   ├── Cargo.toml
│   │   ├── README.md
│   │   ├── ALGORITHM.md
│   │   └── src/
│   │       ├── lib.rs            # Public API and sync() function
│   │       ├── buffer.rs         # Per-stream message buffering
│   │       ├── config.rs         # Configuration types
│   │       ├── staleness.rs      # Staleness detection system
│   │       ├── state.rs          # Core state machine
│   │       ├── sync.rs           # Synchronization logic
│   │       └── types.rs          # Core traits (WithTimestamp, Key)
│   │
│   └── conflux-ros2/             # ROS2 integration utilities (Rust)
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs            # Public API
│           ├── message.rs        # TimestampedMessage, ROS time conversion
│           └── subscriber.rs     # Dynamic subscription utilities
│
├── conflux_node/                 # Standalone ROS2 node (Rust)
│   ├── Cargo.toml
│   ├── package.xml               # ROS2 package manifest
│   ├── config/                   # Configuration files
│   │   ├── example.yaml
│   │   ├── presets/
│   │   └── examples/
│   ├── launch/                   # ROS2 launch files
│   │   └── *.launch.xml
│   └── src/
│       ├── main.rs               # Entry point
│       ├── lib.rs                # Library root
│       ├── config.rs             # YAML configuration parsing
│       └── node.rs               # ConfluxNode implementation
│
├── conflux_cpp/                  # C++ ROS2 library
│   ├── CMakeLists.txt
│   ├── package.xml
│   ├── include/conflux/          # Public C++ headers
│   │   ├── synchronizer.hpp
│   │   ├── types.hpp
│   │   └── visibility.h
│   ├── src/                      # C++ implementation
│   │   ├── synchronizer.cpp
│   │   ├── ffi_bridge.cpp
│   │   └── ffi_bridge.hpp
│   ├── examples/
│   │   └── sync_node.cpp
│   └── rust/                     # FFI crate (built by CMake)
│       ├── Cargo.toml
│       ├── cbindgen.toml
│       └── src/lib.rs
│
├── conflux_py/                   # Python ROS2 library
│   ├── pyproject.toml            # maturin build config
│   ├── package.xml
│   ├── setup.py
│   ├── conflux_py/               # Python package
│   │   ├── __init__.py
│   │   ├── synchronizer.py       # ROS2Synchronizer wrapper
│   │   └── _conflux_py.pyi       # Type stubs
│   ├── examples/
│   │   └── sync_node.py
│   ├── test/
│   │   └── test_synchronizer.py
│   └── rust/                     # PyO3 crate (built by maturin)
│       ├── Cargo.toml
│       └── src/lib.rs
│
├── test/
│   └── *.sh                      # Test scripts
│
├── docs/
│   └── roadmap/                  # Development roadmap
│
└── external/                     # Reference repos (gitignored)
```

## Architecture

### conflux-core (Pure Rust Library)

The core library provides generic stream synchronization with these key components:

- **`sync()` function**: Main entry point that takes a stream of `(Key, Message)` pairs
- **`WithTimestamp` trait**: Implement this for your message type to provide timestamps
- **`Key` trait**: Any type that's `Clone + Eq + Hash + Send + Sync`
- **`State<K, T>`**: Core state machine managing buffers and matching logic
- **`StalenessDetector`**: Prevents indefinite message accumulation

```rust
use conflux_core::{sync, Config, WithTimestamp};

// Your message type just needs to implement WithTimestamp
impl WithTimestamp for MyMessage {
    fn timestamp(&self) -> Duration { self.ts }
}

// Then use sync() to synchronize streams
let config = Config::basic(Duration::from_millis(50), None, 64);
let (output_stream, feedback) = sync(input_stream, keys, config)?;
```

### conflux-ros2 (ROS2 Integration Library)

Provides utilities for integrating conflux-core with ROS2:

- **`TimestampedMessage`**: Message wrapper with extracted timestamp
- **`SynchronizedGroup`**: Collection of synchronized messages
- **`create_dynamic_subscription()`**: Subscribe to any message type at runtime
- **`extract_header_stamp()`**: Extract timestamp from DynamicMessage
- **`ros_time_to_duration()` / `duration_to_ros_time()`**: Time conversion utilities

```rust
use conflux_ros2::{create_dynamic_subscription, TimestampedMessage, SynchronizedGroup};
use conflux_ros2::conflux_core::{sync, Config};

// In your own ROS2 node:
let (tx, rx) = mpsc::unbounded_channel();

// Subscribe to topics dynamically
let _sub = create_dynamic_subscription(
    &node,
    "/camera/image",
    "sensor_msgs/msg/Image",
    qos,
    tx,
)?;

// Messages flow through the channel with timestamps extracted
```

### conflux-node (Standalone ROS2 Node)

The node supports **arbitrary** ROS2 message types at runtime through dynamic type introspection.
This is not a whitelist - any message type works as long as its type support library is installed.

**How it works:**
1. Message type is specified as a string in config (e.g., `"sensor_msgs/msg/Image"`)
2. At runtime, rclrs loads the type support library via `libloading`
3. Message structure is introspected via `rosidl_typesupport_introspection_c`
4. Fields are accessed dynamically using `DynamicMessage::get("field_name")`
5. No recompilation needed when changing message types

**Timestamp extraction:**
The `extract_header_stamp()` function in `crates/conflux-ros2/src/subscriber.rs` navigates the message structure:
```rust
msg.get("header")  // -> Value::Simple(SimpleValue::Message(header_view))
header_view.get("stamp")  // -> Value::Simple(SimpleValue::Message(stamp_view))
stamp_view.get("sec")     // -> Value::Simple(SimpleValue::Int32(&sec))
stamp_view.get("nanosec") // -> Value::Simple(SimpleValue::Uint32(&nanosec))
```

**Requirements:**
- Message must have a `header.stamp` field for timestamp extraction
- If missing, falls back to zero timestamp with a warning
- Type support library must be installed (e.g., `ros-humble-sensor-msgs`)

**Common compatible message types:**
- `sensor_msgs/msg/{Image, PointCloud2, Imu, LaserScan, CameraInfo, ...}`
- `nav_msgs/msg/{Odometry, Path, OccupancyGrid, ...}`
- `geometry_msgs/msg/{PoseStamped, TwistStamped, ...}`
- Any custom message type with a `std_msgs/Header` field

## Configuration

The node requires a YAML configuration file. Users must provide:
- Input topics and their message types
- Output topic
- Synchronization parameters (window_size, buffer_size)

See `config/example.yaml` for full documentation.

## Dependencies

- **conflux-core**: Core synchronization algorithm (workspace crate)
- **conflux-ros2**: ROS2 integration utilities (workspace crate)
- **rclrs**: ROS2 Rust client library (git commit 562e815 for DynamicMessage support)
- **tokio**: Async runtime for synchronization tasks
- **sensor_msgs, std_msgs, builtin_interfaces**: ROS2 message types (resolved via colcon build)

## Build Notes

- ROS2 message crates (`sensor_msgs`, `std_msgs`, etc.) use wildcard versions (`*`) in Cargo.toml
- These are patched at build time via `build/ros2_cargo_config.toml` generated by colcon
- Direct `cargo build` will fail for ROS2 crates - always use `just build`
- `cargo build -p conflux-core` works without ROS2 environment
- The `test-release` profile provides release optimizations with debug symbols

## Launch Files

Launch files (XML format) are installed to `share/conflux/launch/` and can be run with:

```bash
# Basic launch with config file
ros2 launch conflux conflux.launch.xml config:=/path/to/config.yaml

# Real-time synchronization (camera + LiDAR, latency-constrained)
ros2 launch conflux realtime_sync.launch.xml

# Offline processing with rosbag
ros2 launch conflux offline_sync.launch.xml bag:=/path/to/recording

# Multi-sensor fusion (camera + LiDAR + IMU)
ros2 launch conflux multi_sensor_fusion.launch.xml

# Stereo camera (tight 5ms window)
ros2 launch conflux stereo_camera.launch.xml
```

**Scenario comparison:**

| Scenario | Window | Staleness | QoS | Use Case |
|----------|--------|-----------|-----|----------|
| Real-time | 33ms | high_frequency | best_effort | Autonomous driving |
| Offline | 500ms | batch | reliable | Rosbag processing |
| Multi-sensor | 100ms | high_frequency | best_effort | Sensor fusion |
| Stereo | 5ms | high_frequency | best_effort | Depth estimation |
| Low-frequency | 200ms | low_frequency | best_effort | GPS/odometry |

## Testing

All test commands assume `.envrc` is sourced (see Environment Setup).

```bash
# Run core library tests (no ROS2 required)
just test-core

# Run full workspace tests (requires prior build for ROS2 types)
just test

# Test launch files (requires built package)
bash test/test_all_launches.sh

# Quick single launch test
bash test/test_launch_quick.sh
```

**Launch file testing:**
The test scripts use `play_launch` to start nodes and verify:
- Node starts successfully
- Subscriptions are created for expected topics
- No errors in node stderr
