"""Tests for conflux_py synchronization module."""

import pytest


class _FakeNode:
    """Small adapter for driving ROS subscription callbacks without a ROS graph."""

    def __init__(self):
        self.callbacks = {}

    def create_subscription(self, _msg_type, topic, callback, _qos):
        self.callbacks[topic] = callback
        return object()


class _Message:
    def __init__(self, timestamp_ns):
        stamp = type("Stamp", (), {})()
        stamp.sec, stamp.nanosec = divmod(timestamp_ns, 1_000_000_000)
        self.header = type("Header", (), {"stamp": stamp})()


class TestSyncConfig:
    """Tests for SyncConfig class."""

    def test_default_config(self):
        """Test default configuration values."""
        from conflux_py import SyncConfig

        config = SyncConfig()
        assert config.window_size_ms == 50
        assert config.buffer_size == 64

    def test_custom_config(self):
        """Test custom configuration values."""
        from conflux_py import SyncConfig

        config = SyncConfig(window_size_ms=100, buffer_size=32)
        assert config.window_size_ms == 100
        assert config.buffer_size == 32

    def test_config_repr(self):
        """Test configuration string representation."""
        from conflux_py import SyncConfig

        config = SyncConfig(window_size_ms=50, buffer_size=64)
        assert "50" in repr(config)
        assert "64" in repr(config)


class TestSynchronizer:
    """Tests for Synchronizer class."""

    def test_create_synchronizer(self):
        """Test creating a synchronizer."""
        from conflux_py import SyncConfig, Synchronizer

        config = SyncConfig(window_size_ms=100, buffer_size=10)
        sync = Synchronizer(["topic1", "topic2"], config)

        assert sync.topic_count == 2
        assert "topic1" in sync.topics
        assert "topic2" in sync.topics

    def test_create_synchronizer_default_config(self):
        """Test creating a synchronizer with default config."""
        from conflux_py import Synchronizer

        sync = Synchronizer(["topic1", "topic2"])
        assert sync.topic_count == 2

    def test_create_synchronizer_empty_topics(self):
        """Test that empty topics list raises error."""
        from conflux_py import Synchronizer

        with pytest.raises(ValueError, match="empty"):
            Synchronizer([])

    def test_create_synchronizer_invalid_buffer_size(self):
        """buffer_size < 2 is rejected, now at config construction (L-21).

        SyncConfig validates eagerly, so the error surfaces at the point the bad
        value was written rather than later at Synchronizer(). FFISynchronizer
        keeps its own check for callers that bypass SyncConfig.
        """
        from conflux_py import SyncConfig

        with pytest.raises(ValueError, match="buffer_size"):
            SyncConfig(buffer_size=1)

    def test_ffi_layer_also_rejects_invalid_buffer_size(self):
        """The low-level layer validates independently of SyncConfig."""
        from conflux_py._ffi import FFISynchronizer

        with pytest.raises(ValueError, match="buffer_size"):
            FFISynchronizer(["topic1"], buffer_size=1)

    def test_push_valid_message(self):
        """Test pushing a valid message."""
        from conflux_py import Synchronizer

        sync = Synchronizer(["topic1", "topic2"])

        result = sync.push("topic1", 1_000_000_000, {"data": "test"})
        assert result is True

    def test_push_unknown_topic(self):
        """Test pushing to unknown topic raises error."""
        from conflux_py import Synchronizer

        sync = Synchronizer(["topic1"])

        with pytest.raises(KeyError, match="unknown"):
            sync.push("unknown_topic", 1_000_000_000, {"data": "test"})

    def test_push_negative_timestamp(self):
        """Test pushing negative timestamp raises error."""
        from conflux_py import Synchronizer

        sync = Synchronizer(["topic1"])

        with pytest.raises(ValueError, match="non-negative"):
            sync.push("topic1", -1, {"data": "test"})

    def test_poll_no_match(self):
        """Test polling when no match is available."""
        from conflux_py import Synchronizer

        sync = Synchronizer(["topic1", "topic2"])

        # Push only one message - should not match yet
        sync.push("topic1", 1_000_000_000, {"data": "test1"})

        result = sync.poll()
        assert result is None

    def test_poll_with_match(self):
        """Test polling when a match is available."""
        from conflux_py import SyncConfig, Synchronizer

        config = SyncConfig(window_size_ms=100, buffer_size=10)
        sync = Synchronizer(["topic1", "topic2"], config)

        # Push messages with close timestamps
        sync.push("topic1", 1_000_000_000, {"data": "msg1"})
        sync.push("topic2", 1_000_000_000, {"data": "msg2"})

        # Push more messages to enable matching
        sync.push("topic1", 1_100_000_000, {"data": "msg3"})
        sync.push("topic2", 1_100_000_000, {"data": "msg4"})

        # Should get a synchronized group
        result = sync.poll()
        assert result is not None
        assert len(result) == 2
        assert "topic1" in result
        assert "topic2" in result

    def test_sync_group_access(self):
        """Test SyncGroup access methods."""
        from conflux_py import SyncConfig, Synchronizer

        config = SyncConfig(window_size_ms=100, buffer_size=10)
        sync = Synchronizer(["topic1", "topic2"], config)

        # Push messages
        sync.push("topic1", 1_000_000_000, {"data": "msg1"})
        sync.push("topic2", 1_000_000_000, {"data": "msg2"})
        sync.push("topic1", 1_100_000_000, {"data": "msg3"})
        sync.push("topic2", 1_100_000_000, {"data": "msg4"})

        group = sync.poll()
        assert group is not None

        # Test get method
        msg1 = group.get("topic1")
        assert msg1 is not None
        assert msg1["data"] == "msg1"

        # Test subscript access
        msg2 = group["topic2"]
        assert msg2["data"] == "msg2"

        # Test topics method
        topics = group.topics()
        assert set(topics) == {"topic1", "topic2"}

        # Test to_dict
        d = group.to_dict()
        assert "topic1" in d
        assert "topic2" in d

        # Test contains
        assert "topic1" in group
        assert "unknown" not in group

        # Test timestamp
        assert group.timestamp_ns > 0
        assert group.timestamp > 0.0

    def test_sync_group_key_error(self):
        """Test SyncGroup raises KeyError for unknown topic."""
        from conflux_py import SyncConfig, Synchronizer

        config = SyncConfig(window_size_ms=100, buffer_size=10)
        sync = Synchronizer(["topic1", "topic2"], config)

        sync.push("topic1", 1_000_000_000, {"data": "msg1"})
        sync.push("topic2", 1_000_000_000, {"data": "msg2"})
        sync.push("topic1", 1_100_000_000, {"data": "msg3"})
        sync.push("topic2", 1_100_000_000, {"data": "msg4"})

        group = sync.poll()
        assert group is not None

        with pytest.raises(KeyError):
            _ = group["unknown_topic"]

    def test_is_ready(self):
        """Test is_ready method."""
        from conflux_py import Synchronizer

        sync = Synchronizer(["topic1", "topic2"])

        # Initially not ready
        assert sync.is_ready() is False

        # Still not ready with just one message each
        sync.push("topic1", 1_000_000_000, {"data": "msg1"})
        sync.push("topic2", 1_000_000_000, {"data": "msg2"})
        assert sync.is_ready() is False

        # Ready with two messages each
        sync.push("topic1", 1_100_000_000, {"data": "msg3"})
        sync.push("topic2", 1_100_000_000, {"data": "msg4"})
        assert sync.is_ready() is True

    def test_has_empty_buffer(self):
        """has_empty_buffer reports whether ANY topic is missing data (L-17)."""
        from conflux_py import Synchronizer

        sync = Synchronizer(["topic1", "topic2"])

        # Initially every buffer is empty
        assert sync.has_empty_buffer() is True

        # Still true if only one topic has messages -- no group can form
        sync.push("topic1", 1_000_000_000, {"data": "msg1"})
        assert sync.has_empty_buffer() is True

        # False once all topics have messages
        sync.push("topic2", 1_000_000_000, {"data": "msg2"})
        assert sync.has_empty_buffer() is False

    def test_buffer_len(self):
        """Test buffer_len method."""
        from conflux_py import Synchronizer

        sync = Synchronizer(["topic1", "topic2"])

        assert sync.buffer_len("topic1") == 0
        assert sync.buffer_len("topic2") == 0

        sync.push("topic1", 1_000_000_000, {"data": "msg1"})
        assert sync.buffer_len("topic1") == 1
        assert sync.buffer_len("topic2") == 0

        sync.push("topic1", 1_100_000_000, {"data": "msg2"})
        assert sync.buffer_len("topic1") == 2

    def test_drain(self):
        """Test drain method."""
        from conflux_py import SyncConfig, Synchronizer

        config = SyncConfig(window_size_ms=100, buffer_size=10)
        sync = Synchronizer(["topic1", "topic2"], config)

        # Push multiple rounds of messages
        for i in range(3):
            ts = (i + 1) * 1_000_000_000
            sync.push("topic1", ts, {"data": f"msg1_{i}"})
            sync.push("topic2", ts, {"data": f"msg2_{i}"})

        # Drain should return multiple groups
        groups = sync.drain()
        assert len(groups) >= 1

    def test_iterator(self):
        """Test iterator protocol."""
        from conflux_py import SyncConfig, Synchronizer

        config = SyncConfig(window_size_ms=100, buffer_size=10)
        sync = Synchronizer(["topic1", "topic2"], config)

        # Push messages
        for i in range(4):
            ts = (i + 1) * 1_000_000_000
            sync.push("topic1", ts, {"data": f"msg1_{i}"})
            sync.push("topic2", ts, {"data": f"msg2_{i}"})

        # Use iterator
        count = 0
        for group in sync:
            assert len(group) == 2
            count += 1

        assert count >= 1


class TestReset:
    """M-22: recovery from a source clock restart (bag loop, sim-time reset)."""

    def test_reset_revives_stream_after_clock_jump(self):
        from conflux_py import Synchronizer, SyncConfig

        sync = Synchronizer(["A", "B"], SyncConfig(window_size_ms=50, buffer_size=8))
        ms = 1_000_000

        sync.push("A", 5000 * ms, "a1")
        sync.push("B", 5010 * ms, "b1")
        assert sync.poll() is not None

        # Source restarts its clock: every push is refused from here on.
        assert sync.push("A", 1000 * ms, "a2") is False

        sync.reset()

        assert sync.push("A", 1000 * ms, "a2") is True
        assert sync.push("B", 1010 * ms, "b2") is True
        assert sync.poll() is not None

    def test_reset_clears_buffers(self):
        from conflux_py import Synchronizer, SyncConfig

        sync = Synchronizer(["A", "B"], SyncConfig(window_size_ms=50, buffer_size=8))
        sync.push("A", 1_000_000_000, "a1")
        assert sync.buffer_len("A") == 1

        sync.reset()

        assert sync.buffer_len("A") == 0


class TestStatus:
    """M-23: the matcher must be able to explain why it is not emitting."""

    def test_status_waiting_for_data(self):
        from conflux_py import BlockedReason, Synchronizer, SyncConfig

        sync = Synchronizer(["A", "B"], SyncConfig(window_size_ms=50, buffer_size=8))
        status = sync.status
        assert status.blocked is BlockedReason.WAITING_FOR_DATA
        assert status.inf_ts_ns is None
        assert status.is_stalled is False

    def test_status_reports_wedge_shape(self):
        from conflux_py import BlockedReason, Synchronizer, SyncConfig

        ms = 1_000_000
        sync = Synchronizer(["A", "B"], SyncConfig(window_size_ms=50, buffer_size=2))
        for ts in (1000, 1010):
            sync.push("A", ts * ms, f"a{ts}")
        for ts in (5000, 5010):
            sync.push("B", ts * ms, f"b{ts}")

        status = sync.status
        assert status.blocked is BlockedReason.BUFFER_FULL_NO_MATCH
        assert status.shortfall_ns is not None and status.shortfall_ns > 0
        assert status.is_stalled is True

    def test_status_not_blocked_when_group_available(self):
        from conflux_py import Synchronizer, SyncConfig

        ms = 1_000_000
        sync = Synchronizer(["A", "B"], SyncConfig(window_size_ms=50, buffer_size=8))
        sync.push("A", 1000 * ms, "a")
        sync.push("B", 1005 * ms, "b")

        assert sync.status.blocked is None


class TestNamingAndValidation:
    """L-17, L-19, L-20, L-21: the API should say what it means."""

    def test_has_empty_buffer_says_what_it_means(self):
        from conflux_py import Synchronizer

        sync = Synchronizer(["A", "B"])
        assert sync.has_empty_buffer() is True

        sync.push("A", 1_000_000_000, "a")
        assert sync.has_empty_buffer() is True, "B is still empty"
        assert sync.all_buffers_empty() is False, "A holds a message"

        sync.push("B", 1_005_000_000, "b")
        assert sync.has_empty_buffer() is False

    def test_is_empty_alias_still_works(self):
        from conflux_py import Synchronizer

        sync = Synchronizer(["A", "B"])
        sync.push("A", 1_000_000_000, "a")
        with pytest.warns(DeprecationWarning, match="has_empty_buffer"):
            assert sync.is_empty() == sync.has_empty_buffer()

    def test_zero_window_rejected_with_a_pointer_to_none(self):
        from conflux_py import SyncConfig

        with pytest.raises(ValueError, match="None"):
            SyncConfig(window_size_ms=0)

    def test_none_window_means_infinite(self):
        from conflux_py import SyncConfig, Synchronizer

        config = SyncConfig(window_size_ms=None)
        assert config.window_size_ms is None
        # An infinite window must still construct and match.
        sync = Synchronizer(["A", "B"], config)
        sync.push("A", 1_000_000_000, "a")
        sync.push("B", 9_000_000_000, "b")
        assert sync.poll() is not None, "infinite window matches any spread"

    def test_buffer_size_error_explains_the_floor(self):
        from conflux_py import SyncConfig, Synchronizer

        with pytest.raises(ValueError) as exc:
            Synchronizer(["A"], SyncConfig(buffer_size=1))
        message = str(exc.value)
        assert "buffer_size" in message
        assert "2" in message
class TestROS2Synchronizer:
    def test_reset_accepts_a_rewound_timestamp_epoch(self):
        """Reset keeps the ROS wiring but forgets buffered/committed timestamps."""
        from conflux_py import ROS2Synchronizer

        node = _FakeNode()
        sync = ROS2Synchronizer(node, window_size_ms=100, buffer_size=10)
        sync.add_subscription(_Message, "camera")
        sync.add_subscription(_Message, "lidar")
        groups = []
        sync.on_synchronized(groups.append)

        def publish_pair(timestamp_ns):
            node.callbacks["camera"](_Message(timestamp_ns))
            node.callbacks["lidar"](_Message(timestamp_ns))

        publish_pair(100_000_000_000)
        assert len(groups) == 1

        publish_pair(1_000_000_000)
        assert len(groups) == 1

        sync.reset()
        publish_pair(1_000_000_000)

        assert len(groups) == 2
        assert sync.statistics.messages_received == {"camera": 3, "lidar": 3}
