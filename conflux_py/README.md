# conflux_py

Python bindings for the conflux multi-stream message synchronization library.

## Installation

```bash
# Build and install with maturin
maturin build --release
pip install target/wheels/conflux_py-*.whl
```

## Usage

### Low-level API

```python
from conflux_py import Synchronizer, SyncConfig

# Create synchronizer
config = SyncConfig(window_size_ms=50, buffer_size=64)
sync = Synchronizer(["/camera/image", "/lidar/points"], config)

# Push messages
sync.push("/camera/image", timestamp_ns, image_msg)
sync.push("/lidar/points", timestamp_ns, points_msg)

# Poll for synchronized groups
for group in sync:
    image = group["/camera/image"]
    points = group["/lidar/points"]
    process(image, points)
```

### ROS2 Integration

```python
from rclpy.node import Node
from sensor_msgs.msg import Image, PointCloud2
from conflux_py import ROS2Synchronizer

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
            self.process(image, points)
```

## License

MIT OR Apache-2.0
