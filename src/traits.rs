//! Traits for generic message handling.
//!
//! These traits enable the synchronizer to work with arbitrary ROS2 message types
//! by providing a common interface for timestamp extraction.

use std::time::Duration;

/// Trait for messages that have a header with a timestamp.
///
/// This is the primary trait that message types should implement to be
/// synchronizable. Most sensor messages (Image, PointCloud2, Imu, etc.)
/// have a `std_msgs/Header` and can implement this trait.
pub trait HasHeader {
    /// Get the timestamp from the message header.
    fn header_stamp(&self) -> (i32, u32);

    /// Get the frame_id from the message header.
    fn header_frame_id(&self) -> &str;
}

/// Trait for extracting a timestamp from a message.
///
/// This is a more general trait than `HasHeader` - it can be implemented
/// for messages that don't have a standard header but have some other
/// way to provide a timestamp.
pub trait Timestamped {
    /// Extract the timestamp as a Duration from epoch.
    fn timestamp(&self) -> Duration;
}

// Blanket implementation: Any message with a Header is Timestamped
impl<T: HasHeader> Timestamped for T {
    fn timestamp(&self) -> Duration {
        let (sec, nanosec) = self.header_stamp();
        if sec >= 0 {
            Duration::new(sec as u64, nanosec)
        } else {
            Duration::ZERO
        }
    }
}

/// Type-erased message container for runtime polymorphism.
///
/// This allows storing different message types in the same collection
/// while preserving timestamp information.
pub struct AnyMessage {
    /// The timestamp extracted from the message.
    pub timestamp: Duration,

    /// The topic this message came from.
    pub topic: String,

    /// The serialized message data (CDR format).
    pub data: Vec<u8>,

    /// Original ROS timestamp for reconstruction.
    pub ros_stamp: (i32, u32),

    /// The message type name (e.g., "sensor_msgs/msg/Image").
    pub type_name: String,
}

impl AnyMessage {
    /// Create a new AnyMessage from a typed message.
    pub fn from_typed<T: HasHeader + Serialize>(
        msg: &T,
        topic: String,
        type_name: String,
    ) -> Self
    where
        T: serde::Serialize,
    {
        let (sec, nanosec) = msg.header_stamp();
        let timestamp = if sec >= 0 {
            Duration::new(sec as u64, nanosec)
        } else {
            Duration::ZERO
        };

        // For now, we'll just store an empty vec - real serialization
        // would use CDR serialization from rclrs
        Self {
            timestamp,
            topic,
            data: Vec::new(), // TODO: Serialize with CDR
            ros_stamp: (sec, nanosec),
            type_name,
        }
    }

    /// Create from raw components (used when receiving from subscriptions).
    pub fn from_raw(
        topic: String,
        timestamp: Duration,
        ros_stamp: (i32, u32),
        data: Vec<u8>,
        type_name: String,
    ) -> Self {
        Self {
            timestamp,
            topic,
            data,
            ros_stamp,
            type_name,
        }
    }
}

// Placeholder for Serialize trait - in practice, use serde or CDR serialization
pub trait Serialize {}

// =============================================================================
// Implementations for common ROS2 message types
// =============================================================================

/// Macro to implement HasHeader for message types with a standard header field.
#[macro_export]
macro_rules! impl_has_header {
    ($msg_type:ty) => {
        impl $crate::traits::HasHeader for $msg_type {
            fn header_stamp(&self) -> (i32, u32) {
                (self.header.stamp.sec, self.header.stamp.nanosec)
            }

            fn header_frame_id(&self) -> &str {
                &self.header.frame_id
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mock message type for testing
    struct MockHeader {
        stamp: MockTime,
        frame_id: String,
    }

    struct MockTime {
        sec: i32,
        nanosec: u32,
    }

    struct MockMessage {
        header: MockHeader,
    }

    impl HasHeader for MockMessage {
        fn header_stamp(&self) -> (i32, u32) {
            (self.header.stamp.sec, self.header.stamp.nanosec)
        }

        fn header_frame_id(&self) -> &str {
            &self.header.frame_id
        }
    }

    #[test]
    fn test_timestamped_from_header() {
        let msg = MockMessage {
            header: MockHeader {
                stamp: MockTime {
                    sec: 1000,
                    nanosec: 500_000_000,
                },
                frame_id: "base_link".to_string(),
            },
        };

        assert_eq!(msg.timestamp(), Duration::new(1000, 500_000_000));
    }

    #[test]
    fn test_negative_timestamp() {
        let msg = MockMessage {
            header: MockHeader {
                stamp: MockTime {
                    sec: -1,
                    nanosec: 0,
                },
                frame_id: "base_link".to_string(),
            },
        };

        assert_eq!(msg.timestamp(), Duration::ZERO);
    }
}
