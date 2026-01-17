//! ROS2 message wrapper that owns a DynamicMessage.
//!
//! This module provides [`Ros2Message`], which wraps a [`DynamicMessage`] with
//! its extracted timestamp. Unlike [`TimestampedMessage`] which stores serialized
//! bytes, this type owns the actual DynamicMessage for later republishing.
//!
//! This is necessary because `DynamicMessage` does not implement `Clone`, so we
//! cannot use the generic conflux-core synchronization with cloneable messages.
//! Instead, we use move semantics throughout the synchronization pipeline.

use rclrs::DynamicMessage;
use std::time::Duration;

/// A ROS2 message with extracted timestamp, owning the DynamicMessage.
///
/// This wrapper owns the [`DynamicMessage`] directly, allowing it to be
/// buffered during synchronization and later republished without losing
/// message content.
///
/// # Example
///
/// ```ignore
/// use conflux_ros2::Ros2Message;
/// use std::time::Duration;
///
/// // Created from a DynamicMessage in a subscription callback
/// let ros2_msg = Ros2Message::new(
///     "/camera/image".to_string(),
///     Duration::from_secs(1000),
///     dynamic_message,  // Ownership transferred
///     (1000, 0),
/// );
///
/// // Later, extract the message for publishing
/// let msg = ros2_msg.into_message();
/// publisher.publish(msg)?;
/// ```
pub struct Ros2Message {
    /// The topic this message came from.
    pub topic: String,

    /// Timestamp extracted from header.stamp.
    pub timestamp: Duration,

    /// The actual DynamicMessage (owned).
    message: DynamicMessage,

    /// Original ROS stamp (sec, nanosec) for reference.
    pub ros_stamp: (i32, u32),
}

impl Ros2Message {
    /// Create a new Ros2Message by taking ownership of a DynamicMessage.
    pub fn new(
        topic: String,
        timestamp: Duration,
        message: DynamicMessage,
        ros_stamp: (i32, u32),
    ) -> Self {
        Self {
            topic,
            timestamp,
            message,
            ros_stamp,
        }
    }

    /// Consume this wrapper and return the owned DynamicMessage.
    ///
    /// Use this when you need to publish the message.
    pub fn into_message(self) -> DynamicMessage {
        self.message
    }

    /// Get a reference to the DynamicMessage for inspection.
    pub fn message(&self) -> &DynamicMessage {
        &self.message
    }
}

// DynamicMessage is Send + Sync, so Ros2Message can be too
// (String and Duration are also Send + Sync)
