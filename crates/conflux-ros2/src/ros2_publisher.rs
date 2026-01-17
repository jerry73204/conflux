//! Dynamic publisher manager for synchronized output topics.
//!
//! This module provides [`Ros2PublisherManager`], which creates and manages
//! dynamic publishers for outputting synchronized messages. Each input topic
//! gets a corresponding output topic with a configurable suffix (e.g., `_sync`).

use eyre::{Result, WrapErr};
use indexmap::IndexMap;
use rclrs::{DynamicMessage, DynamicPublisher, MessageTypeName, Node, PublisherOptions, QoSProfile};
use tracing::info;

/// Manages dynamic publishers for synchronized output topics.
///
/// Creates one publisher per input topic, with the output topic name being
/// `{input_topic}{suffix}` (e.g., `/camera/image` -> `/camera/image_sync`).
///
/// # Example
///
/// ```ignore
/// let topics_and_types = vec![
///     ("/camera/image".to_string(), "sensor_msgs/msg/Image".to_string()),
///     ("/lidar/points".to_string(), "sensor_msgs/msg/PointCloud2".to_string()),
/// ];
///
/// let manager = Ros2PublisherManager::new(
///     &node,
///     &topics_and_types,
///     "_sync",
///     QoSProfile::sensor_data_default(),
/// )?;
///
/// // Later, publish synchronized messages
/// manager.publish("/camera/image", dynamic_message)?;
/// ```
pub struct Ros2PublisherManager {
    /// Publishers keyed by input topic name.
    /// Value is (publisher, output_topic_name, message_type).
    publishers: IndexMap<String, PublisherEntry>,
}

/// Entry for a single publisher.
struct PublisherEntry {
    publisher: DynamicPublisher,
    output_topic: String,
    msg_type: String,
}

impl Ros2PublisherManager {
    /// Create publishers for each input topic with the given suffix.
    ///
    /// # Arguments
    ///
    /// * `node` - The ROS2 node to create publishers on
    /// * `topics_and_types` - List of (input_topic, message_type) pairs
    /// * `output_suffix` - Suffix to append to input topics for output names
    /// * `qos` - QoS profile for the publishers
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - A message type string is invalid
    /// - Publisher creation fails
    pub fn new(
        node: &Node,
        topics_and_types: &[(String, String)],
        output_suffix: &str,
        qos: QoSProfile,
    ) -> Result<Self> {
        let mut publishers = IndexMap::new();

        for (input_topic, msg_type) in topics_and_types {
            let output_topic = format!("{}{}", input_topic, output_suffix);

            // Parse message type name
            let message_type: MessageTypeName = msg_type
                .as_str()
                .try_into()
                .wrap_err_with(|| format!("Invalid message type format: {}", msg_type))?;

            // Create publisher options
            let mut options = PublisherOptions::new(&output_topic);
            options.qos = qos;

            // Create dynamic publisher
            let publisher = node
                .create_dynamic_publisher(message_type, options)
                .wrap_err_with(|| {
                    format!(
                        "Failed to create dynamic publisher for topic '{}' with type '{}'",
                        output_topic, msg_type
                    )
                })?;

            info!(
                input_topic = %input_topic,
                output_topic = %output_topic,
                msg_type = %msg_type,
                "Created dynamic publisher"
            );

            publishers.insert(
                input_topic.clone(),
                PublisherEntry {
                    publisher,
                    output_topic,
                    msg_type: msg_type.clone(),
                },
            );
        }

        Ok(Self { publishers })
    }

    /// Publish a message to the output topic corresponding to the given input topic.
    ///
    /// # Arguments
    ///
    /// * `input_topic` - The original input topic name (used to look up the publisher)
    /// * `message` - The DynamicMessage to publish (ownership transferred)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - No publisher exists for the given input topic
    /// - Publishing fails
    pub fn publish(&self, input_topic: &str, message: DynamicMessage) -> Result<()> {
        let entry = self
            .publishers
            .get(input_topic)
            .ok_or_else(|| eyre::eyre!("No publisher registered for input topic: {}", input_topic))?;

        entry
            .publisher
            .publish(message)
            .wrap_err_with(|| format!("Failed to publish to topic '{}'", entry.output_topic))?;

        Ok(())
    }

    /// Get the output topic name for a given input topic.
    pub fn get_output_topic(&self, input_topic: &str) -> Option<&str> {
        self.publishers.get(input_topic).map(|e| e.output_topic.as_str())
    }

    /// Get the number of publishers.
    pub fn len(&self) -> usize {
        self.publishers.len()
    }

    /// Check if there are no publishers.
    pub fn is_empty(&self) -> bool {
        self.publishers.is_empty()
    }

    /// Iterate over (input_topic, output_topic, msg_type) tuples.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str, &str)> {
        self.publishers
            .iter()
            .map(|(input, entry)| (input.as_str(), entry.output_topic.as_str(), entry.msg_type.as_str()))
    }
}
