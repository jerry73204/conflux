"""Core synchronization classes using the FFI backend.

This module provides Python-friendly wrappers around the low-level FFI bindings.
"""

from dataclasses import dataclass
from typing import Any, Dict, Iterator, List, Optional


@dataclass
class SyncConfig:
    """Configuration for the synchronizer.

    Attributes:
        window_size_ms: Time window in milliseconds for grouping messages.
        buffer_size: Maximum number of messages to buffer per stream.
    """

    window_size_ms: int = 50
    buffer_size: int = 64

    def __repr__(self) -> str:
        return f"SyncConfig(window_size_ms={self.window_size_ms}, buffer_size={self.buffer_size})"


class SyncGroup:
    """A synchronized group of messages from multiple topics.

    This class provides dictionary-like access to messages by topic name.
    """

    def __init__(self, messages: Dict[str, Any], timestamp_ns: int):
        """Initialize a sync group.

        Args:
            messages: Dictionary mapping topic names to message objects.
            timestamp_ns: The group timestamp in nanoseconds.
        """
        self._messages = messages
        self._timestamp_ns = timestamp_ns

    @property
    def timestamp_ns(self) -> int:
        """Get the timestamp of this synchronized group in nanoseconds."""
        return self._timestamp_ns

    @property
    def timestamp(self) -> float:
        """Get the timestamp of this synchronized group in seconds."""
        return self._timestamp_ns / 1_000_000_000.0

    def get(self, topic: str) -> Optional[Any]:
        """Get a message by topic name.

        Args:
            topic: The topic name.

        Returns:
            The message, or None if not found.
        """
        return self._messages.get(topic)

    def topics(self) -> List[str]:
        """Get all topic names in this group."""
        return list(self._messages.keys())

    def __len__(self) -> int:
        """Get the number of messages in this group."""
        return len(self._messages)

    def __contains__(self, topic: str) -> bool:
        """Check if a topic is in this group."""
        return topic in self._messages

    def __getitem__(self, topic: str) -> Any:
        """Get a message by topic name.

        Args:
            topic: The topic name.

        Returns:
            The message.

        Raises:
            KeyError: If the topic is not in the group.
        """
        if topic not in self._messages:
            raise KeyError(topic)
        return self._messages[topic]

    def to_dict(self) -> Dict[str, Any]:
        """Convert to a dictionary."""
        return self._messages.copy()

    def __repr__(self) -> str:
        topics = list(self._messages.keys())
        return f"SyncGroup(timestamp_ns={self._timestamp_ns}, topics={topics})"


class Synchronizer:
    """Multi-stream message synchronizer.

    The Synchronizer collects messages from multiple streams (identified by topic names)
    and outputs groups of messages that fall within a configurable time window.

    Example:
        >>> config = SyncConfig(window_size_ms=50, buffer_size=64)
        >>> sync = Synchronizer(["/camera/image", "/lidar/points"], config)
        >>>
        >>> sync.push("/camera/image", timestamp_ns, image_msg)
        >>> sync.push("/lidar/points", timestamp_ns, points_msg)
        >>>
        >>> for group in sync:
        ...     image = group["/camera/image"]
        ...     points = group["/lidar/points"]
    """

    def __init__(self, topics: List[str], config: Optional[SyncConfig] = None):
        """Create a new synchronizer.

        Args:
            topics: List of topic names to synchronize.
            config: Optional configuration. Uses defaults if not provided.

        Raises:
            ValueError: If topics list is empty.
            RuntimeError: If the FFI library is not available.
        """
        if not topics:
            raise ValueError("topics list cannot be empty")

        self._topics = list(topics)
        self._config = config or SyncConfig()

        # Try to use FFI backend, fall back to pure Python
        try:
            from ._ffi import FFISynchronizer, is_available

            if is_available():
                self._ffi_sync = FFISynchronizer(
                    topics,
                    window_size_ms=self._config.window_size_ms,
                    buffer_size=self._config.buffer_size,
                )
                self._use_ffi = True
            else:
                self._use_ffi = False
                self._pure_python_init()
        except ImportError:
            self._use_ffi = False
            self._pure_python_init()

    def _pure_python_init(self):
        """Initialize pure Python fallback synchronizer."""
        # Simple buffer-based implementation
        self._buffers: Dict[str, List[tuple]] = {topic: [] for topic in self._topics}
        self._max_buffer = self._config.buffer_size

    def push(self, topic: str, timestamp_ns: int, message: Any) -> bool:
        """Push a message to the synchronizer.

        Args:
            topic: The topic name for this message.
            timestamp_ns: Timestamp in nanoseconds.
            message: The message object.

        Returns:
            True if the message was accepted, False if rejected.

        Raises:
            KeyError: If the topic was not registered at creation.
            ValueError: If timestamp_ns is negative.
        """
        if topic not in self._topics:
            raise KeyError(f"Unknown topic: {topic}")

        if timestamp_ns < 0:
            raise ValueError("timestamp_ns must be non-negative")

        if self._use_ffi:
            return self._ffi_sync.push(topic, timestamp_ns, message)
        else:
            # Pure Python fallback
            buffer = self._buffers[topic]
            if len(buffer) >= self._max_buffer:
                return False
            buffer.append((timestamp_ns, message))
            buffer.sort(key=lambda x: x[0])  # Keep sorted by timestamp
            return True

    def poll(self) -> Optional[SyncGroup]:
        """Poll for a synchronized group of messages.

        Returns:
            A SyncGroup if a synchronized group is available, None otherwise.
        """
        if self._use_ffi:
            result = self._ffi_sync.poll()
            if result:
                # Convert FFI result to SyncGroup
                messages = {topic: msg for topic, (ts, msg) in result.items()}
                min_ts = min(ts for ts, msg in result.values())
                return SyncGroup(messages, min_ts)
            return None
        else:
            # Pure Python fallback - simple approximate sync
            return self._pure_python_poll()

    def _pure_python_poll(self) -> Optional[SyncGroup]:
        """Pure Python polling implementation."""
        # Check if all buffers have at least one message
        if any(len(buf) == 0 for buf in self._buffers.values()):
            return None

        # Get the oldest timestamp from each buffer
        oldest = {topic: buf[0] for topic, buf in self._buffers.items()}

        # Find the reference timestamp (median of oldest timestamps)
        timestamps = [ts for ts, msg in oldest.values()]
        timestamps.sort()
        ref_ts = timestamps[len(timestamps) // 2]

        # Check if all messages are within the window
        window_ns = self._config.window_size_ms * 1_000_000
        all_within_window = all(
            abs(ts - ref_ts) <= window_ns for ts, msg in oldest.values()
        )

        if all_within_window:
            # Create sync group and remove messages from buffers
            messages = {}
            min_ts = float("inf")
            for topic, buf in self._buffers.items():
                ts, msg = buf.pop(0)
                messages[topic] = msg
                min_ts = min(min_ts, ts)
            return SyncGroup(messages, int(min_ts))

        # Messages are not synchronized, drop the oldest one
        oldest_topic = min(oldest.keys(), key=lambda t: oldest[t][0])
        self._buffers[oldest_topic].pop(0)
        return None

    def drain(self) -> List[SyncGroup]:
        """Drain all available synchronized groups.

        Returns:
            A list of all available SyncGroups.
        """
        groups = []
        while True:
            group = self.poll()
            if group is None:
                break
            groups.append(group)
        return groups

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
        if self._use_ffi:
            return self._ffi_sync.is_ready()
        return all(len(buf) >= 2 for buf in self._buffers.values())

    def is_empty(self) -> bool:
        """Check if any buffer is empty."""
        if self._use_ffi:
            return self._ffi_sync.is_empty()
        return any(len(buf) == 0 for buf in self._buffers.values())

    def buffer_len(self, topic: str) -> int:
        """Get the buffer length for a specific topic."""
        if self._use_ffi:
            return self._ffi_sync.buffer_len(topic)
        return len(self._buffers.get(topic, []))

    def __iter__(self) -> Iterator[SyncGroup]:
        """Iterate over synchronized groups."""
        return self

    def __next__(self) -> SyncGroup:
        """Get next synchronized group."""
        group = self.poll()
        if group is None:
            raise StopIteration
        return group

    def __repr__(self) -> str:
        backend = "FFI" if self._use_ffi else "Python"
        return (
            f"Synchronizer(topics={self._topics}, "
            f"window_size_ms={self._config.window_size_ms}, "
            f"buffer_size={self._config.buffer_size}, "
            f"backend={backend})"
        )
