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
from ._ffi import BlockedReason, ConfluxResult, MatchStatus

__all__ = [
    "BlockedReason",
    "ConfluxResult",
    "DropPolicy",
    "MatchStatus",
    "SyncConfig",
    "SyncGroup",
    "Synchronizer",
]

# The ROS2 wrapper is optional: it is only importable where rclpy is installed.
#
# L-19: probe for rclpy specifically rather than wrapping the real import in a
# bare `except ImportError`. The broad form swallowed every ImportError raised
# anywhere inside synchronizer.py -- a typo, a renamed symbol, a half-built
# workspace -- and made ROS2Synchronizer silently vanish, so the user's node
# failed later with a traceback naming conflux_py instead of the real cause.
try:
    import rclpy  # noqa: F401
except ImportError:
    pass
else:
    from .synchronizer import ROS2Synchronizer, SyncStatistics  # noqa: F401

    __all__.extend(["ROS2Synchronizer", "SyncStatistics"])

__version__ = "0.2.0"
