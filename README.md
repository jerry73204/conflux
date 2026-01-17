# Conflux

A ROS2 node for multi-stream message synchronization, written in Rust.

Conflux synchronizes messages from multiple input topics within configurable time windows. It uses the [multi-stream-synchronizer](https://github.com/TODO/multi-stream-synchronizer) algorithm to efficiently group messages by timestamp.

## Features

- **Dynamic message type support**: Works with any ROS2 message type that has a `std_msgs/Header` field (sensor_msgs, nav_msgs, geometry_msgs, custom types, etc.)
- **Configurable time windows**: Define synchronization windows from microseconds to seconds
- **Staleness detection**: Prevents memory buildup when streams have different rates or go offline
- **Multiple presets**: Pre-configured settings for high-frequency sensors, low-frequency sensors, and batch processing
- **Flexible QoS**: Configure reliability and history depth per your requirements

## Requirements

- ROS2 Humble
- Rust (stable)
- [colcon-cargo-ros2](https://github.com/camearle20/colcon-cargo-ros2)
- [direnv](https://direnv.net/) (recommended)

## Installation

1. Clone the repository with submodules:
   ```bash
   git clone --recursive https://github.com/jerry73204/conflux.git
   cd conflux
   ```

2. Allow direnv (sources ROS2 environment automatically):
   ```bash
   direnv allow
   ```

3. Install the colcon-cargo-ros2 extension:
   ```bash
   just setup
   ```

4. Build the package:
   ```bash
   just build
   ```

## Usage

### Basic Usage

Create a configuration file (see `config/example.yaml`) and launch the node:

```bash
ros2 launch conflux conflux.launch.xml config:=/path/to/config.yaml
```

Or run directly:

```bash
ros2 run conflux conflux --ros-args -p config_file:=/path/to/config.yaml
```

### Launch Files

Several pre-configured launch files are available for common scenarios:

```bash
# Real-time camera + LiDAR synchronization (33ms window)
ros2 launch conflux realtime_sync.launch.xml

# Offline rosbag processing (500ms window, reliable QoS)
ros2 launch conflux offline_sync.launch.xml

# Multi-sensor fusion: camera + LiDAR + IMU (100ms window)
ros2 launch conflux multi_sensor_fusion.launch.xml

# Stereo camera synchronization (5ms window)
ros2 launch conflux stereo_camera.launch.xml

# Low-frequency localization: GPS + odometry + magnetometer (200ms window)
ros2 launch conflux low_frequency_localization.launch.xml
```

## Configuration

Configuration is done via YAML files. Here's a minimal example:

```yaml
inputs:
  - topic: /camera/image_raw
    type: sensor_msgs/msg/Image
  - topic: /lidar/points
    type: sensor_msgs/msg/PointCloud2

output:
  topic: /synchronized

sync:
  window_size: 50ms
  buffer_size: 64

staleness:
  preset: high_frequency  # or: low_frequency, batch

qos:
  reliability: best_effort  # or: reliable
  history_depth: 1
```

See `config/example.yaml` for full documentation of all options.

### Staleness Presets

| Preset           | Use Case                  | Window Behavior                   |
|------------------|---------------------------|-----------------------------------|
| `high_frequency` | Sensors @ 10-30Hz         | Tight windows, aggressive cleanup |
| `low_frequency`  | Sensors @ 1-10Hz          | Relaxed windows                   |
| `batch`          | Offline/rosbag processing | Large buffers, lazy cleanup       |

## Development

```bash
# Show all available commands
just

# Build (release with debug symbols)
just build

# Format code
just format

# Run lints
just lint

# Run tests
just test

# Run the node
just run

# Clean build artifacts
just clean
```

## Supported Message Types

Conflux supports any ROS2 message type with a `std_msgs/Header` field, including:

- `sensor_msgs/msg/Image`
- `sensor_msgs/msg/PointCloud2`
- `sensor_msgs/msg/Imu`
- `sensor_msgs/msg/LaserScan`
- `sensor_msgs/msg/CameraInfo`
- `nav_msgs/msg/Odometry`
- `nav_msgs/msg/Path`
- `geometry_msgs/msg/PoseStamped`
- `geometry_msgs/msg/TwistStamped`
- Custom message types with headers

Message types are resolved at runtime via dynamic type introspection, so no recompilation is needed when changing message types.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.
