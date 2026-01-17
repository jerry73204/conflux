//! Generic subscription creation for arbitrary message types.
//!
//! This module provides a way to create subscriptions for different message
//! types based on runtime configuration, while maintaining type safety.

use crate::message::TimestampedMessage;
use crate::traits::HasHeader;
use eyre::{bail, Result, WrapErr};
use rclrs::{Node, QoSProfile, Subscription, SubscriptionOptions};
use tokio::sync::mpsc;
use tracing::{error, info};

/// A type-erased subscription handle.
///
/// This allows storing subscriptions of different message types in a single
/// collection while keeping the subscriptions alive.
pub enum AnySubscription {
    Image(Subscription<sensor_msgs::msg::Image>),
    PointCloud2(Subscription<sensor_msgs::msg::PointCloud2>),
    Imu(Subscription<sensor_msgs::msg::Imu>),
    LaserScan(Subscription<sensor_msgs::msg::LaserScan>),
    CameraInfo(Subscription<sensor_msgs::msg::CameraInfo>),
    CompressedImage(Subscription<sensor_msgs::msg::CompressedImage>),
    NavSatFix(Subscription<sensor_msgs::msg::NavSatFix>),
    Range(Subscription<sensor_msgs::msg::Range>),
    JointState(Subscription<sensor_msgs::msg::JointState>),
    Joy(Subscription<sensor_msgs::msg::Joy>),
}

/// Create a subscription for a typed message and send to the sync channel.
fn create_typed_subscription<M>(
    node: &Node,
    topic: &str,
    qos: QoSProfile,
    tx: mpsc::UnboundedSender<(String, TimestampedMessage)>,
    type_name: &str,
) -> Result<Subscription<M>>
where
    M: rclrs::MessageIDL + HasHeader,
{
    let topic_owned = topic.to_string();
    let type_name_owned = type_name.to_string();

    let mut options = SubscriptionOptions::new(topic);
    options.qos = qos;

    let subscription = node
        .create_subscription::<M, _>(options, move |msg: M| {
            let (sec, nanosec) = msg.header_stamp();
            let timestamp = crate::message::ros_time_to_duration(sec, nanosec);

            // For now, we store an empty data vec - in a full implementation
            // we would serialize the message here
            let timestamped = TimestampedMessage::new(
                topic_owned.clone(),
                timestamp,
                Vec::new(), // TODO: Serialize message data
                (sec, nanosec),
            );

            if let Err(e) = tx.send((topic_owned.clone(), timestamped)) {
                error!(
                    topic = %topic_owned,
                    msg_type = %type_name_owned,
                    error = %e,
                    "Failed to send message to sync channel"
                );
            }
        })
        .wrap_err_with(|| format!("Failed to create subscription for topic: {}", topic))?;

    info!(
        topic = %topic,
        msg_type = %type_name,
        "Created typed subscription"
    );

    Ok(subscription)
}

/// Create subscriptions for all configured input topics.
///
/// This function creates the appropriate typed subscription based on the
/// message type string in the configuration.
pub fn create_subscriptions(
    node: &Node,
    inputs: &[crate::config::InputConfig],
    qos: QoSProfile,
    tx: mpsc::UnboundedSender<(String, TimestampedMessage)>,
) -> Result<Vec<AnySubscription>> {
    let mut subscriptions = Vec::with_capacity(inputs.len());

    for input in inputs {
        let subscription = create_subscription_for_type(
            node,
            &input.topic,
            &input.msg_type,
            qos.clone(),
            tx.clone(),
        )?;
        subscriptions.push(subscription);
    }

    Ok(subscriptions)
}

/// Create a subscription based on the message type string.
fn create_subscription_for_type(
    node: &Node,
    topic: &str,
    msg_type: &str,
    qos: QoSProfile,
    tx: mpsc::UnboundedSender<(String, TimestampedMessage)>,
) -> Result<AnySubscription> {
    // Normalize the message type (handle both "sensor_msgs/msg/Image" and "sensor_msgs/Image")
    let normalized = normalize_msg_type(msg_type);

    match normalized.as_str() {
        "sensor_msgs/msg/Image" => {
            let sub = create_typed_subscription::<sensor_msgs::msg::Image>(
                node, topic, qos, tx, msg_type,
            )?;
            Ok(AnySubscription::Image(sub))
        }
        "sensor_msgs/msg/PointCloud2" => {
            let sub = create_typed_subscription::<sensor_msgs::msg::PointCloud2>(
                node, topic, qos, tx, msg_type,
            )?;
            Ok(AnySubscription::PointCloud2(sub))
        }
        "sensor_msgs/msg/Imu" => {
            let sub = create_typed_subscription::<sensor_msgs::msg::Imu>(
                node, topic, qos, tx, msg_type,
            )?;
            Ok(AnySubscription::Imu(sub))
        }
        "sensor_msgs/msg/LaserScan" => {
            let sub = create_typed_subscription::<sensor_msgs::msg::LaserScan>(
                node, topic, qos, tx, msg_type,
            )?;
            Ok(AnySubscription::LaserScan(sub))
        }
        "sensor_msgs/msg/CameraInfo" => {
            let sub = create_typed_subscription::<sensor_msgs::msg::CameraInfo>(
                node, topic, qos, tx, msg_type,
            )?;
            Ok(AnySubscription::CameraInfo(sub))
        }
        "sensor_msgs/msg/CompressedImage" => {
            let sub = create_typed_subscription::<sensor_msgs::msg::CompressedImage>(
                node, topic, qos, tx, msg_type,
            )?;
            Ok(AnySubscription::CompressedImage(sub))
        }
        "sensor_msgs/msg/NavSatFix" => {
            let sub = create_typed_subscription::<sensor_msgs::msg::NavSatFix>(
                node, topic, qos, tx, msg_type,
            )?;
            Ok(AnySubscription::NavSatFix(sub))
        }
        "sensor_msgs/msg/Range" => {
            let sub = create_typed_subscription::<sensor_msgs::msg::Range>(
                node, topic, qos, tx, msg_type,
            )?;
            Ok(AnySubscription::Range(sub))
        }
        "sensor_msgs/msg/JointState" => {
            let sub = create_typed_subscription::<sensor_msgs::msg::JointState>(
                node, topic, qos, tx, msg_type,
            )?;
            Ok(AnySubscription::JointState(sub))
        }
        "sensor_msgs/msg/Joy" => {
            let sub = create_typed_subscription::<sensor_msgs::msg::Joy>(
                node, topic, qos, tx, msg_type,
            )?;
            Ok(AnySubscription::Joy(sub))
        }
        _ => {
            bail!(
                "Unsupported message type '{}' for topic '{}'. \
                 Supported types: sensor_msgs/msg/{{Image, PointCloud2, Imu, LaserScan, \
                 CameraInfo, CompressedImage, NavSatFix, Range, JointState, Joy}}",
                msg_type,
                topic
            );
        }
    }
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
    }
}
