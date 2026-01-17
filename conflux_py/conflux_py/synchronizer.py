"""High-level synchronization API for ROS2 Python nodes.

This module provides a convenient wrapper around the native Synchronizer
that integrates directly with rclpy for easy use in ROS2 nodes.
"""

from typing import Callable, List, Optional, Type, TypeVar

from rclpy.node import Node
from rclpy.qos import QoSHistoryPolicy, QoSProfile, QoSReliabilityPolicy

from ._conflux_py import SyncConfig, SyncGroup
from ._conflux_py import Synchronizer as _Synchronizer

MsgT = TypeVar("MsgT")


class ROS2Synchronizer:
    """Multi-stream message synchronizer for ROS2.

    This class wraps the native Synchronizer and provides automatic integration
    with rclpy subscriptions and message timestamp extraction.

    Example:
        >>> class MyNode(Node):
        ...     def __init__(self):
        ...         super().__init__("my_node")
        ...
        ...         sync = ROS2Synchronizer(self, window_size_ms=50)
        ...         sync.add_subscription(Image, "/camera/image")
        ...         sync.add_subscription(PointCloud2, "/lidar/points")
        ...
        ...         @sync.on_synchronized
        ...         def callback(group):
        ...             image = group["/camera/image"]
        ...             points = group["/lidar/points"]
        ...             self.process(image, points)
    """

    def __init__(
        self,
        node: Node,
        window_size_ms: int = 50,
        buffer_size: int = 64,
        qos: Optional[QoSProfile] = None,
    ):
        """Initialize the ROS2 synchronizer.

        Args:
            node: The ROS2 node to create subscriptions on.
            window_size_ms: Time window in milliseconds for grouping messages.
            buffer_size: Maximum number of messages to buffer per topic.
            qos: QoS profile for subscriptions. Defaults to best-effort, keep-last(1).
        """
        self._node = node
        self._config = SyncConfig(window_size_ms, buffer_size)
        self._topics: List[str] = []
        self._subscriptions = []
        self._callback: Optional[Callable[[SyncGroup], None]] = None
        self._sync: Optional[_Synchronizer] = None

        if qos is None:
            self._qos = QoSProfile(
                reliability=QoSReliabilityPolicy.BEST_EFFORT,
                history=QoSHistoryPolicy.KEEP_LAST,
                depth=1,
            )
        else:
            self._qos = qos

    def add_subscription(self, msg_type: Type[MsgT], topic: str) -> None:
        """Add a topic to synchronize.

        Args:
            msg_type: The ROS2 message type (e.g., sensor_msgs.msg.Image).
            topic: The topic name to subscribe to.

        Note:
            The message type must have a `header.stamp` field for timestamp extraction.
        """
        self._topics.append(topic)

        def msg_callback(msg, topic=topic):
            if self._sync is not None:
                # Extract timestamp from header
                stamp = msg.header.stamp
                timestamp_ns = stamp.sec * 1_000_000_000 + stamp.nanosec
                self._sync.push(topic, timestamp_ns, msg)
                self._poll()

        sub = self._node.create_subscription(msg_type, topic, msg_callback, self._qos)
        self._subscriptions.append(sub)

    def on_synchronized(
        self, callback: Callable[[SyncGroup], None]
    ) -> Callable[[SyncGroup], None]:
        """Register callback for synchronized message groups.

        Can be used as a decorator:
            @sync.on_synchronized
            def handle_sync(group):
                ...

        Args:
            callback: Function to call with each synchronized group.

        Returns:
            The callback function (for decorator usage).
        """
        self._callback = callback
        # Initialize synchronizer now that we have callback
        if self._sync is None and self._topics:
            self._sync = _Synchronizer(self._topics, self._config)
        return callback

    def _poll(self) -> None:
        """Check for synchronized groups and invoke callback."""
        if self._sync is None or self._callback is None:
            return

        for group in self._sync:
            self._callback(group)

    @property
    def topic_count(self) -> int:
        """Get the number of registered topics."""
        return len(self._topics)

    @property
    def topics(self) -> List[str]:
        """Get the list of registered topics."""
        return self._topics.copy()

    def is_ready(self) -> bool:
        """Check if all buffers have at least 2 messages."""
        if self._sync is None:
            return False
        return self._sync.is_ready()
