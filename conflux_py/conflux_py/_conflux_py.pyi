"""Type stubs for the native _conflux_py module."""

from typing import Any, Dict, Iterator, List, Optional

class SyncConfig:
    """Configuration for the synchronizer."""

    window_size_ms: int
    buffer_size: int

    def __init__(self, window_size_ms: int = 50, buffer_size: int = 64) -> None: ...
    def __repr__(self) -> str: ...

class SyncGroup:
    """A synchronized group of messages from multiple topics."""

    @property
    def timestamp_ns(self) -> int:
        """Timestamp in nanoseconds for the group."""
        ...

    @property
    def timestamp(self) -> float:
        """Timestamp in seconds for the group."""
        ...

    def get(self, topic: str) -> Optional[Any]:
        """Get a message by topic name."""
        ...

    def topics(self) -> List[str]:
        """Get all topic names in this group."""
        ...

    def to_dict(self) -> Dict[str, Any]:
        """Convert to a dictionary."""
        ...

    def __len__(self) -> int: ...
    def __contains__(self, topic: str) -> bool: ...
    def __getitem__(self, topic: str) -> Any: ...
    def __repr__(self) -> str: ...

class Synchronizer:
    """Multi-stream message synchronizer."""

    def __init__(
        self, topics: List[str], config: Optional[SyncConfig] = None
    ) -> None: ...
    def push(self, topic: str, timestamp_ns: int, message: Any) -> bool:
        """Push a message to the synchronizer.

        Args:
            topic: The topic name for this message.
            timestamp_ns: Timestamp in nanoseconds.
            message: The message object.

        Returns:
            True if accepted, False if rejected.

        Raises:
            KeyError: If topic was not registered.
            ValueError: If timestamp_ns is negative.
        """
        ...

    def poll(self) -> Optional[SyncGroup]:
        """Poll for a synchronized group of messages."""
        ...

    def drain(self) -> List[SyncGroup]:
        """Drain all available synchronized groups."""
        ...

    @property
    def topic_count(self) -> int:
        """Number of registered topics."""
        ...

    @property
    def topics(self) -> List[str]:
        """List of registered topics."""
        ...

    def is_ready(self) -> bool:
        """Check if all buffers have at least 2 messages."""
        ...

    def is_empty(self) -> bool:
        """Check if any buffer is empty."""
        ...

    def buffer_len(self, topic: str) -> int:
        """Get the buffer length for a specific topic."""
        ...

    def __iter__(self) -> Iterator[SyncGroup]: ...
    def __next__(self) -> SyncGroup: ...
    def __repr__(self) -> str: ...
