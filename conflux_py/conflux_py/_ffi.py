"""FFI bindings for conflux using ctypes.

This module provides Python bindings to the conflux-ffi C library,
which is built as part of the conflux_cpp package.
"""

import ctypes
import os
from ctypes import (
    CFUNCTYPE,
    POINTER,
    Structure,
    c_bool,
    c_char_p,
    c_int32,
    c_int64,
    c_size_t,
    c_uint64,
    c_void_p,
)
from pathlib import Path
from typing import Optional


# Result codes from the FFI
class ConfluxResult:
    OK = 0
    INVALID_ARGUMENT = 1
    BUFFER_FULL = 2
    KEY_NOT_FOUND = 3
    NULL_POINTER = 4
    INTERNAL_ERROR = 5


class DropPolicy:
    """Policy for handling buffer overflow when pushing new messages."""

    REJECT_NEW = 0
    """Reject new messages when buffer is full.
    Preserves existing data. Suitable for offline/rosbag processing.
    """

    DROP_OLDEST = 1
    """Drop the oldest message to make room for the new one.
    Always accepts new data. Suitable for realtime processing.
    """


class ConfluxConfig(Structure):
    """Configuration for creating a synchronizer."""

    _fields_ = [
        ("window_size_ms", c_uint64),  # Use 0 for infinite window
        ("buffer_size", c_size_t),
        ("drop_policy", c_int32),  # DropPolicy enum value
    ]


# Callback type for poll function
# void (*callback)(const char *key, int64_t timestamp_ns, void *user_data, void *context)
POLL_CALLBACK = CFUNCTYPE(None, c_char_p, c_int64, c_void_p, c_void_p)


def _find_library() -> Optional[str]:
    """Find the conflux-ffi shared library."""
    # Common library names
    lib_names = [
        "libconflux_ffi.so",
        "conflux_ffi.so",
        "libconflux-ffi.so",
    ]

    # Search paths
    search_paths = []

    # Check LD_LIBRARY_PATH
    ld_path = os.environ.get("LD_LIBRARY_PATH", "")
    search_paths.extend(ld_path.split(":"))

    # Check AMENT_PREFIX_PATH for ROS2 installed libraries
    ament_path = os.environ.get("AMENT_PREFIX_PATH", "")
    for prefix in ament_path.split(":"):
        if prefix:
            search_paths.append(os.path.join(prefix, "lib"))

    # Check relative to this file (for development)
    this_dir = Path(__file__).parent.parent.parent
    search_paths.append(str(this_dir / "conflux_cpp" / "rust" / "target" / "release"))

    # Standard system paths
    search_paths.extend(["/usr/lib", "/usr/local/lib", "/lib"])

    for search_path in search_paths:
        if not search_path:
            continue
        for lib_name in lib_names:
            lib_path = os.path.join(search_path, lib_name)
            if os.path.exists(lib_path):
                return lib_path

    return None


# Load the library
_lib_path = _find_library()
_lib: Optional[ctypes.CDLL] = None

if _lib_path:
    try:
        _lib = ctypes.CDLL(_lib_path)

        # Set up function signatures
        _lib.conflux_synchronizer_new.argtypes = [
            POINTER(ConfluxConfig),
            POINTER(c_char_p),
            c_size_t,
        ]
        _lib.conflux_synchronizer_new.restype = c_void_p

        _lib.conflux_synchronizer_free.argtypes = [c_void_p]
        _lib.conflux_synchronizer_free.restype = None

        _lib.conflux_push_message.argtypes = [c_void_p, c_char_p, c_int64, c_void_p]
        _lib.conflux_push_message.restype = c_int32

        _lib.conflux_poll.argtypes = [c_void_p, POLL_CALLBACK, c_void_p]
        _lib.conflux_poll.restype = c_int32

        _lib.conflux_key_count.argtypes = [c_void_p]
        _lib.conflux_key_count.restype = c_size_t

        _lib.conflux_is_ready.argtypes = [c_void_p]
        _lib.conflux_is_ready.restype = c_bool

        _lib.conflux_is_empty.argtypes = [c_void_p]
        _lib.conflux_is_empty.restype = c_bool

        _lib.conflux_buffer_len.argtypes = [c_void_p, c_char_p]
        _lib.conflux_buffer_len.restype = c_size_t

    except OSError:
        _lib = None


def is_available() -> bool:
    """Check if the FFI library is available."""
    return _lib is not None


def get_library_path() -> Optional[str]:
    """Get the path to the loaded library."""
    return _lib_path


class FFISynchronizer:
    """Low-level wrapper around the conflux FFI synchronizer."""

    def __init__(
        self,
        topics: list[str],
        window_size_ms: Optional[int] = 50,
        buffer_size: int = 64,
        drop_policy: int = DropPolicy.REJECT_NEW,
    ):
        """Create a new synchronizer.

        Args:
            topics: List of topic names to synchronize.
            window_size_ms: Time window in milliseconds. Use None or 0 for infinite window.
            buffer_size: Maximum messages to buffer per topic.
            drop_policy: Policy for buffer overflow (DropPolicy.REJECT_NEW or DropPolicy.DROP_OLDEST).

        Raises:
            RuntimeError: If the FFI library is not available.
            ValueError: If topics is empty.
        """
        if not is_available():
            raise RuntimeError(
                "conflux-ffi library not found. Searched paths include AMENT_PREFIX_PATH. "
                "Make sure conflux_cpp is built and installed."
            )

        if not topics:
            raise ValueError("topics list cannot be empty")

        self._topics = list(topics)
        self._handle: Optional[c_void_p] = None

        # Create config (0 means infinite window)
        config = ConfluxConfig(
            window_size_ms=window_size_ms if window_size_ms is not None else 0,
            buffer_size=buffer_size,
            drop_policy=drop_policy,
        )

        # Create key array
        key_array = (c_char_p * len(topics))()
        for i, topic in enumerate(topics):
            key_array[i] = topic.encode("utf-8")

        # Create synchronizer
        self._handle = _lib.conflux_synchronizer_new(
            ctypes.byref(config), key_array, len(topics)
        )

        if not self._handle:
            raise RuntimeError("Failed to create synchronizer")

        # Store references to prevent garbage collection of message objects
        self._message_refs: dict[int, object] = {}
        self._next_id = 0

    def __del__(self):
        """Clean up the synchronizer."""
        if self._handle and _lib:
            _lib.conflux_synchronizer_free(self._handle)
            self._handle = None

    def push(self, topic: str, timestamp_ns: int, message: object) -> bool:
        """Push a message to the synchronizer.

        Args:
            topic: The topic name.
            timestamp_ns: Timestamp in nanoseconds.
            message: The message object (will be stored and returned in poll).

        Returns:
            True if accepted, False if rejected (buffer full).

        Raises:
            KeyError: If the topic was not registered.
        """
        if not self._handle:
            raise RuntimeError("Synchronizer has been freed")

        # Store message reference and get an ID
        msg_id = self._next_id
        self._next_id += 1
        self._message_refs[msg_id] = message

        result = _lib.conflux_push_message(
            self._handle,
            topic.encode("utf-8"),
            timestamp_ns,
            ctypes.c_void_p(msg_id),
        )

        if result == ConfluxResult.KEY_NOT_FOUND:
            del self._message_refs[msg_id]
            raise KeyError(f"Unknown topic: {topic}")
        elif result == ConfluxResult.BUFFER_FULL:
            del self._message_refs[msg_id]
            return False
        elif result != ConfluxResult.OK:
            del self._message_refs[msg_id]
            return False

        return True

    def poll(self) -> Optional[dict[str, tuple[int, object]]]:
        """Poll for a synchronized group.

        Returns:
            A dictionary mapping topic names to (timestamp_ns, message) tuples,
            or None if no synchronized group is available.
        """
        if not self._handle:
            raise RuntimeError("Synchronizer has been freed")

        result_group: dict[str, tuple[int, object]] = {}
        msg_ids_to_remove: list[int] = []

        def callback(key: bytes, timestamp_ns: int, user_data: int, context: c_void_p):
            topic = key.decode("utf-8")
            msg_id = user_data
            if msg_id in self._message_refs:
                message = self._message_refs[msg_id]
                result_group[topic] = (timestamp_ns, message)
                msg_ids_to_remove.append(msg_id)

        cb = POLL_CALLBACK(callback)
        poll_result = _lib.conflux_poll(self._handle, cb, None)

        # Clean up returned message references
        for msg_id in msg_ids_to_remove:
            del self._message_refs[msg_id]

        if poll_result == 1:
            return result_group
        return None

    @property
    def topics(self) -> list[str]:
        """Get the list of registered topics."""
        return self._topics.copy()

    @property
    def topic_count(self) -> int:
        """Get the number of registered topics."""
        if not self._handle:
            return 0
        return _lib.conflux_key_count(self._handle)

    def is_ready(self) -> bool:
        """Check if all buffers have at least 2 messages."""
        if not self._handle:
            return False
        return _lib.conflux_is_ready(self._handle)

    def is_empty(self) -> bool:
        """Check if any buffer is empty."""
        if not self._handle:
            return True
        return _lib.conflux_is_empty(self._handle)

    def buffer_len(self, topic: str) -> int:
        """Get the buffer length for a specific topic."""
        if not self._handle:
            return 0
        return _lib.conflux_buffer_len(self._handle, topic.encode("utf-8"))
