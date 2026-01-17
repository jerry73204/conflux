//! MsyncNode implementation.

use crate::config::{Config, Reliability};
use crate::message::{SynchronizedGroup, TimestampedMessage};
use crate::subscriber::{create_subscriptions, AnySubscription};
use eyre::{Result, WrapErr};
use futures::stream::{self, TryStreamExt};
use indexmap::IndexMap;
use multi_stream_synchronizer::sync;
use rclrs::{Node, QoSHistoryPolicy, QoSProfile, QoSReliabilityPolicy};
use tokio::sync::mpsc;
use tracing::{debug, info};

/// The multi-stream synchronization node.
pub struct MsyncNode {
    /// The ROS2 node handle.
    _node: Node,

    /// Subscriptions for input topics (kept alive).
    /// Uses type-erased subscriptions to support multiple message types.
    _subscriptions: Vec<AnySubscription>,

    /// Channel receiver for incoming messages.
    message_rx: mpsc::UnboundedReceiver<(String, TimestampedMessage)>,

    /// Configuration.
    config: Config,
}

impl MsyncNode {
    /// Create a new MsyncNode with the given configuration.
    pub fn new(node: Node, config: Config) -> Result<Self> {
        let (message_tx, message_rx) = mpsc::unbounded_channel();

        // Build QoS profile from config
        let qos = build_qos_profile(&config);

        // Create subscriptions for all input topics using the generic factory
        let subscriptions = create_subscriptions(&node, &config.inputs, qos, message_tx)?;

        info!(
            num_subscriptions = subscriptions.len(),
            "Created subscriptions for all input topics"
        );

        Ok(Self {
            _node: node,
            _subscriptions: subscriptions,
            message_rx,
            config,
        })
    }

    /// Run the synchronization loop.
    ///
    /// This consumes the node and runs until the input stream ends or an error occurs.
    pub async fn run(self) -> Result<()> {
        info!(
            window_size = ?self.config.sync.window_size,
            buffer_size = self.config.sync.buffer_size,
            num_inputs = self.config.inputs.len(),
            "Starting synchronization"
        );

        // Extract fields we need
        let keys: Vec<String> = self.config.inputs.iter().map(|i| i.topic.clone()).collect();
        let sync_config = self.config.to_sync_config();
        let message_rx = self.message_rx;

        // Convert mpsc receiver to a stream (boxed for Unpin)
        let input_stream = Box::pin(stream::unfold(message_rx, |mut rx| async move {
            rx.recv().await.map(|msg| (Ok(msg), rx))
        }));

        // Run synchronization
        let (output_stream, _feedback_rx) = sync(input_stream, keys, sync_config)
            .wrap_err("Failed to create synchronization stream")?;

        // Process synchronized groups
        let result: Result<(), eyre::Error> = output_stream
            .try_for_each(|group: IndexMap<String, TimestampedMessage>| async move {
                let timestamp = group.values().next().map(|m| m.timestamp).unwrap_or_default();
                let synced = SynchronizedGroup::new(timestamp, group);

                handle_synchronized_group(synced);
                Ok(())
            })
            .await;

        result.wrap_err("Synchronization stream ended with error")
    }
}

/// Handle a synchronized group of messages.
fn handle_synchronized_group(group: SynchronizedGroup) {
    info!(
        timestamp = ?group.timestamp,
        num_messages = group.len(),
        topics = ?group.messages.keys().collect::<Vec<_>>(),
        "Synchronized group"
    );

    // Log details for each message in the group
    for (topic, msg) in group.iter() {
        debug!(
            topic = %topic,
            timestamp = ?msg.timestamp,
            data_len = msg.data.len(),
            "Message in group"
        );
    }

    // TODO: Publish synchronized output
    // This would serialize the group and publish to the output topic
}

/// Build a QoS profile from the configuration.
fn build_qos_profile(config: &Config) -> QoSProfile {
    let mut qos = QoSProfile::sensor_data_default();

    qos.history = QoSHistoryPolicy::KeepLast {
        depth: config.qos.history_depth as u32,
    };

    qos.reliability = match config.qos.reliability {
        Reliability::BestEffort => QoSReliabilityPolicy::BestEffort,
        Reliability::Reliable => QoSReliabilityPolicy::Reliable,
    };

    qos
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SyncConfig;
    use std::time::Duration;

    #[test]
    fn test_build_qos_profile() {
        let config = Config {
            inputs: vec![],
            output: crate::config::OutputConfig {
                topic: "/out".to_string(),
            },
            sync: SyncConfig {
                window_size: Duration::from_millis(50),
                buffer_size: 64,
            },
            staleness: None,
            qos: crate::config::QosConfig {
                reliability: Reliability::BestEffort,
                history_depth: 5,
            },
        };

        let qos = build_qos_profile(&config);
        assert_eq!(qos.reliability, QoSReliabilityPolicy::BestEffort);
        assert_eq!(qos.history, QoSHistoryPolicy::KeepLast { depth: 5_u32 });
    }
}
