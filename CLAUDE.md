# CLAUDE.md

This file provides guidance to Claude Code when working with this repository.

## Environment Setup

**IMPORTANT: Ensure `.envrc` is sourced before working on this repository.**

This project uses [direnv](https://direnv.net/) to automatically source the ROS2 environment. The `.envrc` file:
- Sources `/opt/ros/humble/setup.bash` (ROS2 Humble)
- Sources `install/setup.bash` when available (built workspace)

Run `direnv allow` in the project directory if not already done. All commands in the justfile assume the environment is already set up.

## Project Overview

msync is a ROS2 node written in Rust that synchronizes messages from multiple input topics within configurable time windows. It uses the `multi-stream-synchronizer` library (included as a git submodule) to perform the synchronization algorithm.

## Build System

This project uses `colcon-cargo-ros2` to build Rust code as a ROS2 package.

**IMPORTANT: Always use the justfile for building and development tasks.**

```bash
# Build the project (ALWAYS use this)
just build

# Never run colcon or cargo commands directly for building
# The justfile handles proper ROS2 environment setup and configuration
```

## Common Development Commands

```bash
# Show available commands
just

# Install colcon-cargo-ros2 extension
just setup

# Build with colcon (primary build method)
just build

# Build with verbose output
just build-verbose

# Build in debug mode (faster compilation)
just build-debug

# Format code
just format

# Check formatting without changes
just format-check

# Run lints (requires prior build)
just lint

# Run all checks (format-check + lint)
just check

# Run tests with nextest
just test

# Run tests with cargo test
just test-cargo

# Run only library tests
just test-lib

# Run the node (after build)
just run

# Clean all artifacts
just clean

# Clean only Rust target/
just clean-rust

# Clean only colcon build/install/log
just clean-colcon

# Update git submodules
just submodules
```

## Project Structure

```
msync/
├── Cargo.toml              # Rust package manifest
├── package.xml             # ROS2 package manifest
├── justfile                # Build and development commands
├── .envrc                  # direnv config (sources ROS2)
├── config/
│   ├── example.yaml        # Documented configuration example
│   ├── presets/            # Pre-configured settings
│   │   ├── high_frequency.yaml
│   │   ├── low_frequency.yaml
│   │   └── batch_processing.yaml
│   └── examples/           # Scenario-specific configs
│       ├── realtime_camera_lidar.yaml
│       ├── offline_exact_match.yaml
│       ├── multi_sensor_fusion.yaml
│       ├── low_frequency_localization.yaml
│       └── stereo_camera.yaml
├── launch/
│   ├── msync.launch.xml             # Basic launch (config file arg)
│   ├── realtime_sync.launch.xml     # Real-time latency-constrained
│   ├── offline_sync.launch.xml      # Offline rosbag processing
│   ├── multi_sensor_fusion.launch.xml
│   ├── low_frequency_localization.launch.xml
│   └── stereo_camera.launch.xml
├── src/
│   ├── main.rs             # Entry point
│   ├── lib.rs              # Library root
│   ├── config.rs           # Configuration parsing
│   ├── message.rs          # TimestampedMessage wrapper
│   ├── node.rs             # MsyncNode implementation
│   └── subscriber.rs       # Dynamic subscription factory
├── test/
│   ├── test_all_launches.sh    # Tests all launch files
│   ├── test_basic_launch.sh    # Tests basic launch file
│   └── test_launch_quick.sh    # Quick single launch test
├── multi-stream-synchronizer/  # Submodule with sync algorithm
└── external/               # Reference repos (gitignored)
```

## Architecture

### Dynamic Message Handling

The node supports **any** ROS2 message type at runtime through dynamic type introspection.
This is enabled by using rclrs's `DynamicMessage` and `DynamicSubscription` functionality
from commit 562e815 (not yet released in v0.6.0).

**How it works:**
1. Message type is specified as a string in config (e.g., `"sensor_msgs/msg/Image"`)
2. At runtime, rclrs loads the type support library via `libloading`
3. Message structure is introspected via `rosidl_typesupport_introspection_c`
4. Fields are accessed dynamically using `DynamicMessage::get("field_name")`

**Key types from rclrs:**
- `DynamicMessage`: Runtime message instance with field access
- `DynamicSubscription`: Subscription that receives `DynamicMessage` instead of typed messages
- `MessageTypeName`: Parsed message type (package/msg/type)
- `Value`/`SimpleValue`: Enum for accessing field values

**Timestamp extraction:**
The `extract_header_stamp()` function navigates the message structure:
```rust
msg.get("header")  // -> Value::Simple(SimpleValue::Message(header_view))
header_view.get("stamp")  // -> Value::Simple(SimpleValue::Message(stamp_view))
stamp_view.get("sec")     // -> Value::Simple(SimpleValue::Int32(&sec))
stamp_view.get("nanosec") // -> Value::Simple(SimpleValue::Uint32(&nanosec))
```

**Supported message types:**
Any ROS2 message with a `std_msgs/Header` field, including:
- All `sensor_msgs/msg/*` types with headers
- All `nav_msgs/msg/*` types with headers
- All `geometry_msgs/msg/*Stamped` types
- Custom message types with headers

**Reference implementations:**
- `external/rclrs`: rclrs source (v0.6.0 + commit 562e815)
- `external/message_filters`: C++ message_filters for design patterns

## Configuration

The node requires a YAML configuration file. Users must provide:
- Input topics and their message types
- Output topic
- Synchronization parameters (window_size, buffer_size)

See `config/example.yaml` for full documentation.

## Dependencies

- **multi-stream-synchronizer**: Core synchronization algorithm (local submodule)
- **rclrs**: ROS2 Rust client library (git commit 562e815 for DynamicMessage support)
- **tokio**: Async runtime for synchronization tasks
- **sensor_msgs, std_msgs, builtin_interfaces**: ROS2 message types (resolved via colcon build)

## Build Notes

- ROS2 message crates (`sensor_msgs`, `std_msgs`, etc.) use wildcard versions (`*`) in Cargo.toml
- These are patched at build time via `build/ros2_cargo_config.toml` generated by colcon
- Direct `cargo build` will fail - always use `just build`
- The `test-release` profile provides release optimizations with debug symbols

## Launch Files

Launch files (XML format) are installed to `share/msync/launch/` and can be run with:

```bash
# Basic launch with config file
ros2 launch msync msync.launch.xml config:=/path/to/config.yaml

# Real-time synchronization (camera + LiDAR, latency-constrained)
ros2 launch msync realtime_sync.launch.xml

# Offline processing with rosbag
ros2 launch msync offline_sync.launch.xml bag:=/path/to/recording

# Multi-sensor fusion (camera + LiDAR + IMU)
ros2 launch msync multi_sensor_fusion.launch.xml

# Stereo camera (tight 5ms window)
ros2 launch msync stereo_camera.launch.xml
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
# Run all tests (requires prior build for ROS2 types)
just test

# Run only the multi-stream-synchronizer library tests
just test-lib

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
