//! Message wrapper types for synchronization.

use multi_stream_synchronizer::WithTimestamp;
use std::time::Duration;

/// A timestamped message wrapper that can hold any ROS message data.
///
/// This wrapper extracts the timestamp from ROS message headers and provides
/// it to the multi-stream-synchronizer.
#[derive(Debug, Clone)]
pub struct TimestampedMessage {
    /// The topic this message came from.
    pub topic: String,

    /// Timestamp extracted from the message header.
    pub timestamp: Duration,

    /// The serialized message data.
    pub data: Vec<u8>,

    /// Original ROS timestamp (sec, nanosec) for reconstruction.
    pub ros_stamp: (i32, u32),
}

impl TimestampedMessage {
    /// Create a new timestamped message.
    pub fn new(topic: String, timestamp: Duration, data: Vec<u8>, ros_stamp: (i32, u32)) -> Self {
        Self {
            topic,
            timestamp,
            data,
            ros_stamp,
        }
    }

    /// Create from a ROS header timestamp.
    ///
    /// Converts ROS time (sec, nanosec) to a Duration from epoch.
    pub fn from_ros_time(topic: String, sec: i32, nanosec: u32, data: Vec<u8>) -> Self {
        let timestamp = ros_time_to_duration(sec, nanosec);
        Self {
            topic,
            timestamp,
            data,
            ros_stamp: (sec, nanosec),
        }
    }
}

impl WithTimestamp for TimestampedMessage {
    fn timestamp(&self) -> Duration {
        self.timestamp
    }
}

/// Convert ROS time (sec, nanosec) to Duration.
///
/// ROS2 uses builtin_interfaces/Time with:
/// - sec: int32 (seconds since epoch, can be negative for pre-1970)
/// - nanosec: uint32 (nanoseconds component, 0-999999999)
pub fn ros_time_to_duration(sec: i32, nanosec: u32) -> Duration {
    if sec >= 0 {
        Duration::new(sec as u64, nanosec)
    } else {
        // Handle negative seconds (pre-1970 timestamps) - rare in practice
        // For synchronization purposes, we use Duration::ZERO as floor
        Duration::ZERO
    }
}

/// Convert Duration back to ROS time components.
pub fn duration_to_ros_time(duration: Duration) -> (i32, u32) {
    let secs = duration.as_secs();
    let nanos = duration.subsec_nanos();

    // Saturate at i32::MAX for safety
    let sec = if secs > i32::MAX as u64 {
        i32::MAX
    } else {
        secs as i32
    };

    (sec, nanos)
}

/// A synchronized group of messages from multiple topics.
#[derive(Debug, Clone)]
pub struct SynchronizedGroup {
    /// The reference timestamp for this group.
    pub timestamp: Duration,

    /// Messages in this group, keyed by topic name.
    pub messages: indexmap::IndexMap<String, TimestampedMessage>,
}

impl SynchronizedGroup {
    /// Create a new synchronized group.
    pub fn new(timestamp: Duration, messages: indexmap::IndexMap<String, TimestampedMessage>) -> Self {
        Self { timestamp, messages }
    }

    /// Get a message by topic name.
    pub fn get(&self, topic: &str) -> Option<&TimestampedMessage> {
        self.messages.get(topic)
    }

    /// Number of messages in the group.
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// Check if the group is empty.
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// Iterate over topic-message pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &TimestampedMessage)> {
        self.messages.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ros_time_conversion() {
        // Normal case
        let duration = ros_time_to_duration(1000, 500_000_000);
        assert_eq!(duration, Duration::new(1000, 500_000_000));

        // Zero
        let duration = ros_time_to_duration(0, 0);
        assert_eq!(duration, Duration::ZERO);

        // Negative (floors to zero)
        let duration = ros_time_to_duration(-1, 0);
        assert_eq!(duration, Duration::ZERO);
    }

    #[test]
    fn test_duration_to_ros_time() {
        let (sec, nanosec) = duration_to_ros_time(Duration::new(1000, 500_000_000));
        assert_eq!(sec, 1000);
        assert_eq!(nanosec, 500_000_000);
    }

    #[test]
    fn test_timestamped_message() {
        let msg = TimestampedMessage::from_ros_time(
            "/camera".to_string(),
            1000,
            500_000_000,
            vec![1, 2, 3],
        );

        assert_eq!(msg.topic, "/camera");
        assert_eq!(msg.timestamp(), Duration::new(1000, 500_000_000));
        assert_eq!(msg.ros_stamp, (1000, 500_000_000));
    }
}
