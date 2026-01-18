//! Configuration parsing and validation for the conflux node.

use eyre::{Result, WrapErr, bail, ensure};
use serde::Deserialize;
use std::{fs, path::Path, time::Duration};

/// Root configuration structure.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// Input topics to synchronize.
    pub inputs: Vec<InputConfig>,

    /// Output topic configuration.
    pub output: OutputConfig,

    /// Synchronization parameters.
    pub sync: SyncConfig,

    /// Optional staleness detection configuration.
    #[serde(default)]
    pub staleness: Option<StalenessConfig>,

    /// Optional QoS configuration.
    #[serde(default)]
    pub qos: QosConfig,
}

impl Config {
    /// Load configuration from a YAML file.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let contents = fs::read_to_string(path)
            .wrap_err_with(|| format!("Failed to read config file: {}", path.display()))?;

        let config: Config = serde_yaml::from_str(&contents)
            .wrap_err_with(|| format!("Failed to parse config file: {}", path.display()))?;

        config.validate()?;
        Ok(config)
    }

    /// Validate the configuration.
    fn validate(&self) -> Result<()> {
        ensure!(
            !self.inputs.is_empty(),
            "At least one input topic is required"
        );

        ensure!(
            !self.output.suffix.is_empty(),
            "Output suffix cannot be empty"
        );

        ensure!(
            !self.sync.window_size.is_zero(),
            "sync.window_size must be greater than zero"
        );

        ensure!(
            self.sync.buffer_size > 0,
            "sync.buffer_size must be greater than zero"
        );

        // Validate input topics
        for (i, input) in self.inputs.iter().enumerate() {
            ensure!(
                !input.topic.is_empty(),
                "Input topic at index {} cannot be empty",
                i
            );
            ensure!(
                !input.msg_type.is_empty(),
                "Input type at index {} cannot be empty",
                i
            );
        }

        // Check for duplicate topics
        let mut topics: Vec<&str> = self.inputs.iter().map(|i| i.topic.as_str()).collect();
        topics.sort();
        for window in topics.windows(2) {
            if window[0] == window[1] {
                bail!("Duplicate input topic: {}", window[0]);
            }
        }

        // Staleness config uses presets, no additional validation needed

        Ok(())
    }

    /// Convert to conflux-core Config.
    pub fn to_sync_config(&self) -> conflux_ros2::conflux_core::Config {
        use conflux_ros2::conflux_core;

        let staleness_config = self.staleness.as_ref().map(|s| match s.preset {
            StalenessPreset::HighFrequency => {
                conflux_core::StalenessConfig::high_frequency()
            }
            StalenessPreset::LowFrequency => {
                conflux_core::StalenessConfig::low_frequency()
            }
            StalenessPreset::Batch => {
                conflux_core::StalenessConfig::batch_processing()
            }
        });

        let drop_policy = match self.sync.drop_policy {
            DropPolicySetting::RejectNew => conflux_core::DropPolicy::RejectNew,
            DropPolicySetting::DropOldest => conflux_core::DropPolicy::DropOldest,
        };

        conflux_core::Config {
            window_size: Some(self.sync.window_size),
            start_time: None,
            buf_size: self.sync.buffer_size,
            drop_policy,
            staleness_config,
        }
    }
}

/// Configuration for an input topic.
#[derive(Debug, Clone, Deserialize)]
pub struct InputConfig {
    /// The ROS2 topic name.
    pub topic: String,

    /// The message type (e.g., "sensor_msgs/msg/Image").
    #[serde(rename = "type")]
    pub msg_type: String,
}

/// Configuration for output topics.
#[derive(Debug, Clone, Deserialize)]
pub struct OutputConfig {
    /// Suffix to append to input topics for output (e.g., "_sync").
    /// Each input topic `/foo` will have synchronized output at `/foo_sync`.
    #[serde(default = "default_output_suffix")]
    pub suffix: String,
}

fn default_output_suffix() -> String {
    "_sync".to_string()
}

/// Synchronization parameters.
#[derive(Debug, Clone, Deserialize)]
pub struct SyncConfig {
    /// Time window for grouping messages.
    #[serde(with = "humantime_serde")]
    pub window_size: Duration,

    /// Maximum messages to buffer per input stream.
    pub buffer_size: usize,

    /// Policy for handling buffer overflow.
    #[serde(default)]
    pub drop_policy: DropPolicySetting,
}

/// Drop policy configuration setting.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DropPolicySetting {
    /// Reject new messages when buffer is full.
    /// Preserves existing data. Suitable for offline/rosbag processing.
    #[default]
    RejectNew,
    /// Drop the oldest message to make room for the new one.
    /// Always accepts new data. Suitable for realtime processing.
    DropOldest,
}

/// Staleness detection configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct StalenessConfig {
    /// Preset configuration: "high_frequency", "low_frequency", or "batch"
    pub preset: StalenessPreset,
}

/// Staleness preset options.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StalenessPreset {
    /// High-frequency streaming (sub-millisecond precision, real-time)
    #[default]
    HighFrequency,
    /// Low-frequency streaming (millisecond precision, near real-time)
    LowFrequency,
    /// Batch processing (relaxed precision, lazy checking)
    Batch,
}

/// QoS configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct QosConfig {
    /// Reliability setting.
    #[serde(default = "default_reliability")]
    pub reliability: Reliability,

    /// History depth.
    #[serde(default = "default_history_depth")]
    pub history_depth: usize,
}

impl Default for QosConfig {
    fn default() -> Self {
        Self {
            reliability: default_reliability(),
            history_depth: default_history_depth(),
        }
    }
}

fn default_reliability() -> Reliability {
    Reliability::BestEffort
}

fn default_history_depth() -> usize {
    1
}

/// QoS reliability setting.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Reliability {
    #[default]
    BestEffort,
    Reliable,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_example_config() {
        let yaml = r#"
inputs:
  - topic: /camera/image
    type: sensor_msgs/msg/Image
  - topic: /lidar/points
    type: sensor_msgs/msg/PointCloud2

output:
  suffix: _sync

sync:
  window_size: 50ms
  buffer_size: 64

staleness:
  preset: high_frequency

qos:
  reliability: best_effort
  history_depth: 1
"#;

        let config: Config = serde_yaml::from_str(yaml).unwrap();
        config.validate().unwrap();

        assert_eq!(config.inputs.len(), 2);
        assert_eq!(config.output.suffix, "_sync");
        assert_eq!(config.sync.window_size, Duration::from_millis(50));
        assert_eq!(config.sync.buffer_size, 64);
        assert_eq!(config.sync.drop_policy, DropPolicySetting::RejectNew);
        assert_eq!(
            config.staleness.as_ref().unwrap().preset,
            StalenessPreset::HighFrequency
        );
    }

    #[test]
    fn test_drop_oldest_policy() {
        let yaml = r#"
inputs:
  - topic: /camera/image
    type: sensor_msgs/msg/Image

output:
  suffix: _sync

sync:
  window_size: 50ms
  buffer_size: 2
  drop_policy: drop_oldest
"#;

        let config: Config = serde_yaml::from_str(yaml).unwrap();
        config.validate().unwrap();

        assert_eq!(config.sync.window_size, Duration::from_millis(50));
        assert_eq!(config.sync.buffer_size, 2);
        assert_eq!(config.sync.drop_policy, DropPolicySetting::DropOldest);
    }

    #[test]
    fn test_reject_empty_inputs() {
        let yaml = r#"
inputs: []
output:
  suffix: _sync
sync:
  window_size: 50ms
  buffer_size: 64
"#;

        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_reject_duplicate_topics() {
        let yaml = r#"
inputs:
  - topic: /camera
    type: sensor_msgs/msg/Image
  - topic: /camera
    type: sensor_msgs/msg/Image
output:
  suffix: _sync
sync:
  window_size: 50ms
  buffer_size: 64
"#;

        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_default_output_suffix() {
        let yaml = r#"
inputs:
  - topic: /camera
    type: sensor_msgs/msg/Image
output: {}
sync:
  window_size: 50ms
  buffer_size: 64
"#;

        let config: Config = serde_yaml::from_str(yaml).unwrap();
        config.validate().unwrap();
        assert_eq!(config.output.suffix, "_sync");
    }
}
