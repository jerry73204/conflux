"""High-level synchronization API for ROS2 Python nodes.

This module provides a convenient wrapper around the native Synchronizer
that integrates directly with rclpy for easy use in ROS2 nodes.
"""

from dataclasses import dataclass, field
from typing import Callable, Dict, List, Optional, Type, TypeVar

from rclpy.node import Node
from rclpy.qos import QoSHistoryPolicy, QoSProfile, QoSReliabilityPolicy

from ._core import DropPolicy, SyncConfig, SyncGroup, Synchronizer as _Synchronizer

MsgT = TypeVar("MsgT")


@dataclass
class SyncStatistics:
    """Statistics for synchronization operations."""

    messages_received: Dict[str, int] = field(default_factory=dict)
    """Count of messages received per topic."""

    messages_rejected: Dict[str, int] = field(default_factory=dict)
    """Count of messages rejected due to buffer overflow per topic."""

    groups_synchronized: int = 0
    """Count of synchronized groups produced."""

    def total_received(self) -> int:
        """Get total messages received across all topics."""
        return sum(self.messages_received.values())

    def total_rejected(self) -> int:
        """Get total messages rejected across all topics."""
        return sum(self.messages_rejected.values())

    def rejection_rate(self, topic: Optional[str] = None) -> float:
        """Get rejection rate (0.0 to 1.0) for a topic or overall.

        Args:
            topic: Topic name, or None for overall rate.

        Returns:
            Rejection rate as a fraction (0.0 = no rejections, 1.0 = all rejected).
        """
        if topic is not None:
            received = self.messages_received.get(topic, 0)
            rejected = self.messages_rejected.get(topic, 0)
        else:
            received = self.total_received()
            rejected = self.total_rejected()

        if received == 0:
            return 0.0
        return rejected / received


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
        window_size_ms: Optional[int] = 50,
        buffer_size: int = 64,
        drop_policy: DropPolicy = DropPolicy.REJECT_NEW,
        qos: Optional[QoSProfile] = None,
        log_overflow: bool = True,
        log_overflow_interval: float = 5.0,
    ):
        """Initialize the ROS2 synchronizer.

        Args:
            node: The ROS2 node to create subscriptions on.
            window_size_ms: Time window in milliseconds for grouping messages.
                Use None for infinite window (no time-based dropping).
            buffer_size: Maximum number of messages to buffer per topic.
            drop_policy: Policy for buffer overflow (DropPolicy.REJECT_NEW or DropPolicy.DROP_OLDEST).
            qos: QoS profile for subscriptions. Defaults to best-effort, keep-last(1).
            log_overflow: Whether to log warnings when buffer overflow occurs.
            log_overflow_interval: Minimum interval in seconds between overflow log messages per topic.
        """
        self._node = node
        self._config = SyncConfig(window_size_ms, buffer_size, drop_policy)
        self._topics: List[str] = []
        self._subscriptions = []
        self._callback: Optional[Callable[[SyncGroup], None]] = None
        self._sync: Optional[_Synchronizer] = None

        # Statistics and logging
        self._stats = SyncStatistics()
        self._log_overflow = log_overflow
        self._log_overflow_interval = log_overflow_interval
        self._last_overflow_log_time: Dict[str, float] = {}

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
        # Initialize statistics for this topic
        self._stats.messages_received[topic] = 0
        self._stats.messages_rejected[topic] = 0

        def msg_callback(msg, topic=topic):
            if self._sync is not None:
                # Extract timestamp from header
                stamp = msg.header.stamp
                timestamp_ns = stamp.sec * 1_000_000_000 + stamp.nanosec

                # Track statistics
                self._stats.messages_received[topic] += 1

                # Push to synchronizer and check if accepted
                accepted = self._sync.push(topic, timestamp_ns, msg)

                if not accepted:
                    self._stats.messages_rejected[topic] += 1
                    self._log_buffer_overflow(topic)

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
            self._stats.groups_synchronized += 1
            self._callback(group)

    def _log_buffer_overflow(self, topic: str) -> None:
        """Log a buffer overflow warning with rate limiting."""
        if not self._log_overflow:
            return

        import time

        now = time.time()
        last_log = self._last_overflow_log_time.get(topic, 0.0)

        if now - last_log >= self._log_overflow_interval:
            self._last_overflow_log_time[topic] = now
            rejected = self._stats.messages_rejected[topic]
            received = self._stats.messages_received[topic]
            rate = rejected / received if received > 0 else 0.0

            policy_name = self._config.drop_policy.name
            self._node.get_logger().warn(
                f"Buffer overflow on '{topic}': {rejected}/{received} messages rejected "
                f"({rate:.1%}), policy={policy_name}, buffer_size={self._config.buffer_size}"
            )

    @property
    def topic_count(self) -> int:
        """Get the number of registered topics."""
        return len(self._topics)

    @property
    def topics(self) -> List[str]:
        """Get the list of registered topics."""
        return self._topics.copy()

    @property
    def statistics(self) -> SyncStatistics:
        """Get synchronization statistics.

        Returns:
            SyncStatistics with message counts and rejection rates.
        """
        return self._stats

    def is_ready(self) -> bool:
        """Check if all buffers have at least 2 messages."""
        if self._sync is None:
            return False
        return self._sync.is_ready()
