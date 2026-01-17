//! Complete ROS2 synchronization node runner.
//!
//! This module provides [`Ros2SyncRunner`], which ties together subscriptions,
//! synchronization state, and publishers to create a complete message
//! synchronization pipeline.

use std::time::Duration;

use eyre::Result;
use indexmap::IndexMap;
use rclrs::{Node, QoSProfile};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::ros2_message::Ros2Message;
use crate::ros2_publisher::Ros2PublisherManager;
use crate::ros2_sync_state::{Ros2SyncState, SyncStats};
use crate::subscriber::{create_ros2_subscription, DynamicSubscriptionHandle};

/// Configuration for the ROS2 synchronization runner.
#[derive(Debug, Clone)]
pub struct Ros2SyncConfig {
    /// Input topics and their message types: `(topic, msg_type)`.
    pub inputs: Vec<(String, String)>,

    /// Suffix to append to input topics for output (e.g., "_sync").
    pub output_suffix: String,

    /// Time window for grouping messages.
    pub window_size: Duration,

    /// Maximum messages to buffer per topic.
    pub buffer_size: usize,

    /// QoS profile for subscriptions and publishers.
    pub qos: QoSProfile,
}

impl Default for Ros2SyncConfig {
    fn default() -> Self {
        Self {
            inputs: Vec::new(),
            output_suffix: "_sync".to_string(),
            window_size: Duration::from_millis(50),
            buffer_size: 64,
            qos: QoSProfile::sensor_data_default(),
        }
    }
}

/// ROS2 message synchronization runner.
///
/// This struct manages the complete synchronization pipeline:
/// 1. Subscriptions receive messages from input topics
/// 2. Messages are buffered and synchronized by timestamp
/// 3. Synchronized groups are published to output topics
///
/// # Example
///
/// ```ignore
/// let config = Ros2SyncConfig {
///     inputs: vec![
///         ("/camera/image".to_string(), "sensor_msgs/msg/Image".to_string()),
///         ("/lidar/points".to_string(), "sensor_msgs/msg/PointCloud2".to_string()),
///     ],
///     output_suffix: "_sync".to_string(),
///     window_size: Duration::from_millis(50),
///     buffer_size: 64,
///     qos: QoSProfile::sensor_data_default(),
/// };
///
/// let runner = Ros2SyncRunner::new(&node, config)?;
/// runner.run().await?;
/// ```
pub struct Ros2SyncRunner {
    /// Subscriptions (kept alive).
    _subscriptions: Vec<DynamicSubscriptionHandle>,

    /// Publishers for output topics.
    publishers: Ros2PublisherManager,

    /// Synchronization state.
    state: Ros2SyncState,

    /// Channel receiver for incoming messages.
    rx: mpsc::UnboundedReceiver<Ros2Message>,

    /// Configuration (for logging/debugging).
    config: Ros2SyncConfig,
}

impl Ros2SyncRunner {
    /// Create a new synchronization runner.
    ///
    /// This creates subscriptions for all input topics and publishers for
    /// all output topics (input topic + suffix).
    pub fn new(node: &Node, config: Ros2SyncConfig) -> Result<Self> {
        let (tx, rx) = mpsc::unbounded_channel();

        // Create subscriptions for all input topics
        let mut subscriptions = Vec::with_capacity(config.inputs.len());
        for (topic, msg_type) in &config.inputs {
            let sub = create_ros2_subscription(node, topic, msg_type, config.qos, tx.clone())?;
            subscriptions.push(sub);
        }

        info!(
            num_subscriptions = subscriptions.len(),
            "Created subscriptions for input topics"
        );

        // Create publishers for all output topics
        let publishers =
            Ros2PublisherManager::new(node, &config.inputs, &config.output_suffix, config.qos)?;

        info!(
            num_publishers = publishers.len(),
            output_suffix = %config.output_suffix,
            "Created publishers for output topics"
        );

        // Create synchronization state
        let topics: Vec<String> = config.inputs.iter().map(|(t, _)| t.clone()).collect();
        let state = Ros2SyncState::new(topics, config.window_size, config.buffer_size);

        info!(
            window_size = ?config.window_size,
            buffer_size = config.buffer_size,
            num_topics = state.num_topics(),
            "Initialized synchronization state"
        );

        Ok(Self {
            _subscriptions: subscriptions,
            publishers,
            state,
            rx,
            config,
        })
    }

    /// Run the synchronization loop.
    ///
    /// This consumes the runner and processes messages until the input
    /// channel is closed or an unrecoverable error occurs.
    pub async fn run(mut self) -> Result<()> {
        info!(
            num_inputs = self.config.inputs.len(),
            window_size = ?self.config.window_size,
            "Starting ROS2 synchronization loop"
        );

        let mut last_stats_log = std::time::Instant::now();
        let stats_log_interval = Duration::from_secs(10);

        while let Some(msg) = self.rx.recv().await {
            let topic = msg.topic.clone();
            let timestamp = msg.timestamp;

            // Push message to synchronization state
            match self.state.push(msg) {
                Ok(()) => {
                    debug!(
                        topic = %topic,
                        timestamp = ?timestamp,
                        "Message pushed to sync state"
                    );
                }
                Err(rejected) => {
                    warn!(
                        topic = %rejected.topic,
                        timestamp = ?rejected.timestamp,
                        commit_ts = ?self.state.commit_timestamp(),
                        "Rejected late message"
                    );
                    continue;
                }
            }

            // Try to match and publish synchronized groups
            while let Some(group) = self.state.try_match() {
                self.publish_group(group)?;
            }

            // Periodically log statistics
            if last_stats_log.elapsed() >= stats_log_interval {
                self.log_stats();
                last_stats_log = std::time::Instant::now();
            }
        }

        // Log final statistics
        info!("Synchronization loop ended");
        self.log_stats();

        Ok(())
    }

    /// Publish a synchronized group of messages.
    fn publish_group(&self, group: IndexMap<String, Ros2Message>) -> Result<()> {
        let num_messages = group.len();
        let timestamps: Vec<_> = group.values().map(|m| m.timestamp).collect();

        debug!(
            num_messages = num_messages,
            timestamps = ?timestamps,
            "Publishing synchronized group"
        );

        for (input_topic, ros2_msg) in group {
            let dynamic_msg = ros2_msg.into_message();

            if let Err(e) = self.publishers.publish(&input_topic, dynamic_msg) {
                warn!(
                    topic = %input_topic,
                    error = %e,
                    "Failed to publish synchronized message"
                );
                // Continue with other messages in the group
            }
        }

        Ok(())
    }

    /// Log current statistics.
    fn log_stats(&self) {
        let stats = self.state.stats();
        info!(
            groups_emitted = stats.groups_emitted,
            late_rejected = stats.late_rejected,
            dropped = stats.dropped,
            commit_ts = ?stats.commit_ts,
            buffer_sizes = ?stats.buffer_sizes,
            "Synchronization statistics"
        );
    }

    /// Get current synchronization statistics.
    pub fn stats(&self) -> SyncStats {
        self.state.stats()
    }
}
