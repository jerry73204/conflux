//! Dynamic subscription creation for arbitrary message types at runtime.
//!
//! This module provides runtime message type support using rclrs's DynamicMessage
//! and DynamicSubscription functionality. Messages are subscribed to based on
//! their type name string (e.g., "sensor_msgs/msg/Image") without requiring
//! compile-time type information.

use crate::message::TimestampedMessage;
use eyre::{Result, WrapErr};
use rclrs::{
    DynamicMessage, DynamicSubscription, MessageInfo, MessageTypeName, Node, QoSProfile,
    SimpleValue, SubscriptionOptions, Value,
};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

/// A type-erased subscription handle for dynamic message types.
///
/// This wraps the rclrs DynamicSubscription and keeps it alive while
/// the node is running.
pub struct DynamicSubscriptionHandle {
    /// The underlying dynamic subscription (kept alive by Arc).
    _subscription: DynamicSubscription,
    /// The message type for logging/debugging.
    pub msg_type: String,
    /// The topic name.
    pub topic: String,
}

/// Extract the header timestamp from a DynamicMessage.
///
/// This navigates the message structure to find:
/// - `header.stamp.sec` (i32)
/// - `header.stamp.nanosec` (u32)
///
/// Returns `None` if the message doesn't have a standard header.
fn extract_header_stamp(msg: &DynamicMessage) -> Option<(i32, u32)> {
    // Get the header field - should be Simple(Message(...))
    let header = msg.get("header")?;

    let Value::Simple(SimpleValue::Message(header_view)) = header else {
        warn!("header field is not a simple message type");
        return None;
    };

    // Get the stamp field from header
    let stamp = header_view.get("stamp")?;

    let Value::Simple(SimpleValue::Message(stamp_view)) = stamp else {
        warn!("stamp field is not a simple message type");
        return None;
    };

    // Get sec and nanosec
    let sec = stamp_view.get("sec")?;
    let nanosec = stamp_view.get("nanosec")?;

    let Value::Simple(SimpleValue::Int32(sec)) = sec else {
        warn!("sec field is not Int32");
        return None;
    };

    let Value::Simple(SimpleValue::Uint32(nanosec)) = nanosec else {
        warn!("nanosec field is not Uint32");
        return None;
    };

    Some((*sec, *nanosec))
}

/// Create a dynamic subscription for any message type at runtime.
fn create_dynamic_subscription(
    node: &Node,
    topic: &str,
    msg_type: &str,
    qos: QoSProfile,
    tx: mpsc::UnboundedSender<(String, TimestampedMessage)>,
) -> Result<DynamicSubscriptionHandle> {
    // Parse the message type name
    let message_type: MessageTypeName = msg_type
        .try_into()
        .wrap_err_with(|| format!("Invalid message type format: {}", msg_type))?;

    let topic_owned = topic.to_string();
    let msg_type_owned = msg_type.to_string();

    // Create subscription options
    let mut options = SubscriptionOptions::new(topic);
    options.qos = qos;

    // Create the dynamic subscription
    let subscription = node
        .create_dynamic_subscription(
            message_type,
            options,
            move |msg: DynamicMessage, _info: MessageInfo| {
                // Extract timestamp from header
                let (sec, nanosec) = match extract_header_stamp(&msg) {
                    Some(stamp) => stamp,
                    None => {
                        warn!(
                            topic = %topic_owned,
                            msg_type = %msg_type_owned,
                            "Message has no header.stamp, using zero timestamp"
                        );
                        (0, 0)
                    }
                };

                let timestamp = crate::message::ros_time_to_duration(sec, nanosec);

                // Create timestamped message
                // Note: We store empty data for now - serialization can be added later
                let timestamped = TimestampedMessage::new(
                    topic_owned.clone(),
                    timestamp,
                    Vec::new(), // TODO: Serialize message data if needed
                    (sec, nanosec),
                );

                if let Err(e) = tx.send((topic_owned.clone(), timestamped)) {
                    error!(
                        topic = %topic_owned,
                        msg_type = %msg_type_owned,
                        error = %e,
                        "Failed to send message to sync channel"
                    );
                }
            },
        )
        .wrap_err_with(|| {
            format!(
                "Failed to create dynamic subscription for topic '{}' with type '{}'",
                topic, msg_type
            )
        })?;

    info!(
        topic = %topic,
        msg_type = %msg_type,
        "Created dynamic subscription"
    );

    Ok(DynamicSubscriptionHandle {
        _subscription: subscription,
        msg_type: msg_type.to_string(),
        topic: topic.to_string(),
    })
}

/// Create subscriptions for all configured input topics.
///
/// This function creates dynamic subscriptions based on the message type
/// strings in the configuration. Any ROS2 message type can be used as long
/// as the corresponding type support library is installed.
pub fn create_subscriptions(
    node: &Node,
    inputs: &[crate::config::InputConfig],
    qos: QoSProfile,
    tx: mpsc::UnboundedSender<(String, TimestampedMessage)>,
) -> Result<Vec<DynamicSubscriptionHandle>> {
    let mut subscriptions = Vec::with_capacity(inputs.len());

    for input in inputs {
        // Normalize message type to full form
        let msg_type = normalize_msg_type(&input.msg_type);

        let subscription =
            create_dynamic_subscription(node, &input.topic, &msg_type, qos, tx.clone())?;
        subscriptions.push(subscription);
    }

    Ok(subscriptions)
}

/// Normalize message type to the full form (package/msg/Type).
fn normalize_msg_type(msg_type: &str) -> String {
    // If already in full form, return as-is
    if msg_type.contains("/msg/") {
        return msg_type.to_string();
    }

    // Convert "sensor_msgs/Image" to "sensor_msgs/msg/Image"
    if let Some((package, type_name)) = msg_type.split_once('/') {
        format!("{}/msg/{}", package, type_name)
    } else {
        msg_type.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_msg_type() {
        assert_eq!(
            normalize_msg_type("sensor_msgs/msg/Image"),
            "sensor_msgs/msg/Image"
        );
        assert_eq!(
            normalize_msg_type("sensor_msgs/Image"),
            "sensor_msgs/msg/Image"
        );
        assert_eq!(
            normalize_msg_type("nav_msgs/msg/Odometry"),
            "nav_msgs/msg/Odometry"
        );
        assert_eq!(
            normalize_msg_type("custom_msgs/CustomType"),
            "custom_msgs/msg/CustomType"
        );
    }
}
