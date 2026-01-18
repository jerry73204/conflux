"""Core synchronization classes using the FFI backend.

This module provides Python-friendly wrappers around the low-level FFI bindings.
"""

from dataclasses import dataclass
from enum import IntEnum
from typing import Any, Dict, Iterator, List, Optional


class DropPolicy(IntEnum):
    """Policy for handling buffer overflow when pushing new messages."""

    REJECT_NEW = 0
    """Reject new messages when buffer is full.
    Preserves existing data. Suitable for offline/rosbag processing.
    """

    DROP_OLDEST = 1
    """Drop the oldest message to make room for the new one.
    Always accepts new data. Suitable for realtime processing.
    """


@dataclass
class SyncConfig:
    """Configuration for the synchronizer.

    Attributes:
        window_size_ms: Time window in milliseconds for grouping messages.
            Use None for infinite window (no time-based dropping).
        buffer_size: Maximum number of messages to buffer per stream.
        drop_policy: Policy for buffer overflow (DropPolicy.REJECT_NEW or DropPolicy.DROP_OLDEST).
    """

    window_size_ms: Optional[int] = 50
    buffer_size: int = 64
    drop_policy: DropPolicy = DropPolicy.REJECT_NEW

    @classmethod
    def offline(cls, buffer_size: int = 100) -> "SyncConfig":
        """Create config for offline processing (rosbag playback).

        Uses infinite window and RejectNew policy to preserve all data.
        """
        return cls(
            window_size_ms=None,
            buffer_size=buffer_size,
            drop_policy=DropPolicy.REJECT_NEW,
        )

    @classmethod
    def realtime(cls, window_size_ms: int = 50, buffer_size: int = 2) -> "SyncConfig":
        """Create config for realtime processing (live sensors).

        Uses finite window and DropOldest policy to always process latest data.
        """
        return cls(
            window_size_ms=window_size_ms,
            buffer_size=buffer_size,
            drop_policy=DropPolicy.DROP_OLDEST,
        )

    def __repr__(self) -> str:
        return (
            f"SyncConfig(window_size_ms={self.window_size_ms}, "
            f"buffer_size={self.buffer_size}, drop_policy={self.drop_policy.name})"
        )


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

        # Use FFI backend (required)
        from ._ffi import FFISynchronizer, is_available

        if not is_available():
            raise RuntimeError(
                "FFI library (libconflux_ffi.so) is not available. "
                "Please build the conflux_ffi crate first."
            )

        self._ffi_sync = FFISynchronizer(
            topics,
            window_size_ms=self._config.window_size_ms,
            buffer_size=self._config.buffer_size,
            drop_policy=int(self._config.drop_policy),
        )

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

        return self._ffi_sync.push(topic, timestamp_ns, message)

    def poll(self) -> Optional[SyncGroup]:
        """Poll for a synchronized group of messages.

        Returns:
            A SyncGroup if a synchronized group is available, None otherwise.
        """
        result = self._ffi_sync.poll()
        if result:
            # Convert FFI result to SyncGroup
            messages = {topic: msg for topic, (ts, msg) in result.items()}
            min_ts = min(ts for ts, msg in result.values())
            return SyncGroup(messages, min_ts)
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
        return self._ffi_sync.is_ready()

    def is_empty(self) -> bool:
        """Check if any buffer is empty."""
        return self._ffi_sync.is_empty()

    def buffer_len(self, topic: str) -> int:
        """Get the buffer length for a specific topic."""
        return self._ffi_sync.buffer_len(topic)

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
        return (
            f"Synchronizer(topics={self._topics}, "
            f"window_size_ms={self._config.window_size_ms}, "
            f"buffer_size={self._config.buffer_size}, "
            f"drop_policy={self._config.drop_policy.name})"
        )
