//! ConfluxNode implementation.

use crate::config::{Config, Reliability};
use conflux_ros2::{Ros2SyncConfig, Ros2SyncRunner};
use eyre::Result;
use rclrs::{Node, QoSHistoryPolicy, QoSProfile, QoSReliabilityPolicy};
use tracing::info;

/// The multi-stream synchronization node.
///
/// This node receives messages from multiple input topics, synchronizes them
/// by timestamp using a time-window algorithm, and publishes the synchronized
/// messages to output topics (input topic + suffix).
pub struct ConfluxNode {
    /// The ROS2 synchronization runner.
    runner: Ros2SyncRunner,
}

impl ConfluxNode {
    /// Create a new ConfluxNode with the given configuration.
    pub fn new(node: &Node, config: Config) -> Result<Self> {
        // Build QoS profile from config
        let qos = build_qos_profile(&config);

        // Build inputs as (topic, msg_type) pairs
        let inputs: Vec<(String, String)> = config
            .inputs
            .iter()
            .map(|i| (i.topic.clone(), i.msg_type.clone()))
            .collect();

        info!(
            num_inputs = inputs.len(),
            output_suffix = %config.output.suffix,
            window_size = ?config.sync.window_size,
            buffer_size = config.sync.buffer_size,
            "Creating ConfluxNode"
        );

        // Create the ROS2 sync configuration
        let sync_config = Ros2SyncConfig {
            inputs,
            output_suffix: config.output.suffix.clone(),
            window_size: config.sync.window_size,
            buffer_size: config.sync.buffer_size,
            qos,
        };

        // Create the synchronization runner
        let runner = Ros2SyncRunner::new(node, sync_config)?;

        Ok(Self { runner })
    }

    /// Run the synchronization loop.
    ///
    /// This consumes the node and runs until the input stream ends or an error occurs.
    pub async fn run(self) -> Result<()> {
        self.runner.run().await
    }
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
    use crate::config::{InputConfig, OutputConfig, QosConfig, SyncConfig};
    use std::time::Duration;

    #[test]
    fn test_build_qos_profile() {
        let config = Config {
            inputs: vec![InputConfig {
                topic: "/test".to_string(),
                msg_type: "std_msgs/msg/String".to_string(),
            }],
            output: OutputConfig {
                suffix: "_sync".to_string(),
            },
            sync: SyncConfig {
                window_size: Duration::from_millis(50),
                buffer_size: 64,
            },
            staleness: None,
            qos: QosConfig {
                reliability: Reliability::BestEffort,
                history_depth: 5,
            },
        };

        let qos = build_qos_profile(&config);
        assert_eq!(qos.reliability, QoSReliabilityPolicy::BestEffort);
        assert_eq!(qos.history, QoSHistoryPolicy::KeepLast { depth: 5_u32 });
    }
}
