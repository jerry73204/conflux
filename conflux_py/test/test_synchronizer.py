"""Tests for conflux_py synchronization module."""

import pytest


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
        """Test that buffer_size < 2 raises error."""
        from conflux_py import SyncConfig, Synchronizer

        config = SyncConfig(buffer_size=1)
        with pytest.raises(ValueError, match="buffer_size"):
            Synchronizer(["topic1"], config)

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

    def test_is_empty(self):
        """Test is_empty method."""
        from conflux_py import Synchronizer

        sync = Synchronizer(["topic1", "topic2"])

        # Initially empty
        assert sync.is_empty() is True

        # Still empty if only one topic has messages
        sync.push("topic1", 1_000_000_000, {"data": "msg1"})
        assert sync.is_empty() is True

        # Not empty when all topics have messages
        sync.push("topic2", 1_000_000_000, {"data": "msg2"})
        assert sync.is_empty() is False

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
