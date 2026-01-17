#!/usr/bin/env python3
"""Example ROS2 node using conflux_py for synchronization.

This node demonstrates how to use the ROS2Synchronizer to synchronize
messages from a camera and LiDAR sensor.

Usage:
    ros2 run conflux_py sync_node

License: MIT OR Apache-2.0
"""

import rclpy
from rclpy.node import Node
from sensor_msgs.msg import Image, PointCloud2

from conflux_py import ROS2Synchronizer


class SyncProcessorNode(Node):
    """Example node that synchronizes camera and LiDAR data."""

    def __init__(self):
        super().__init__("sync_processor")

        # Create synchronizer with 50ms window
        self.sync = ROS2Synchronizer(self, window_size_ms=50)

        # Add topics to synchronize
        self.sync.add_subscription(Image, "/camera/image")
        self.sync.add_subscription(PointCloud2, "/lidar/points")

        # Register callback using decorator
        @self.sync.on_synchronized
        def on_sync(group):
            image = group["/camera/image"]
            points = group["/lidar/points"]

            self.get_logger().info(
                f"Synchronized: image={image.header.stamp.sec}.{image.header.stamp.nanosec:09d}, "
                f"points={points.header.stamp.sec}.{points.header.stamp.nanosec:09d}"
            )

            self.process(image, points)

    def process(self, image: Image, points: PointCloud2):
        """Process synchronized sensor data.

        This is where you would implement your sensor fusion logic.

        Args:
            image: Synchronized camera image.
            points: Synchronized LiDAR point cloud.
        """
        # Your processing logic here
        pass


def main(args=None):
    """Entry point for the sync_node example."""
    rclpy.init(args=args)
    node = SyncProcessorNode()

    try:
        rclpy.spin(node)
    except KeyboardInterrupt:
        pass
    finally:
        node.destroy_node()
        rclpy.shutdown()


if __name__ == "__main__":
    main()
