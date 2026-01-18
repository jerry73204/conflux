"""Conflux - Multi-stream message synchronization library for ROS2.

This package provides Python bindings for the conflux synchronization algorithm,
enabling ROS2 Python nodes to synchronize messages from multiple topics within
configurable time windows.

Example:
    >>> from conflux_py import Synchronizer, SyncConfig
    >>>
    >>> # Create synchronizer with custom config
    >>> config = SyncConfig(window_size_ms=50, buffer_size=64)
    >>> sync = Synchronizer(["/camera/image", "/lidar/points"], config)
    >>>
    >>> # Push messages
    >>> sync.push("/camera/image", timestamp_ns, image_msg)
    >>> sync.push("/lidar/points", timestamp_ns, points_msg)
    >>>
    >>> # Poll for synchronized groups
    >>> for group in sync:
    ...     image = group["/camera/image"]
    ...     points = group["/lidar/points"]
    ...     process(image, points)
"""

from ._core import DropPolicy, SyncConfig, SyncGroup, Synchronizer

__all__ = ["DropPolicy", "SyncConfig", "SyncGroup", "Synchronizer"]

# Conditionally import ROS2Synchronizer if rclpy is available
try:
    from .synchronizer import ROS2Synchronizer  # noqa: F401

    __all__.append("ROS2Synchronizer")
except ImportError:
    pass

__version__ = "0.2.0"
