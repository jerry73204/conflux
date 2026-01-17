//! msync - Multi-stream message synchronization ROS2 node.
//!
//! This library provides a ROS2 node that synchronizes messages from multiple
//! input topics within configurable time windows using the
//! [multi-stream-synchronizer](https://crates.io/crates/multi-stream-synchronizer) algorithm.
//!
//! # Configuration
//!
//! The node requires a YAML configuration file specifying:
//! - Input topics and their message types
//! - Output topic for synchronized batches
//! - Synchronization parameters (window size, buffer size)
//! - Optional staleness detection settings
//! - Optional QoS configuration
//!
//! See the `config/example.yaml` file for a complete example.
//!
//! # Supported Message Types
//!
//! The following message types are supported out of the box:
//! - `sensor_msgs/msg/Image`
//! - `sensor_msgs/msg/PointCloud2`
//! - `sensor_msgs/msg/Imu`
//! - `sensor_msgs/msg/LaserScan`
//! - `sensor_msgs/msg/CameraInfo`
//! - `sensor_msgs/msg/CompressedImage`
//! - `sensor_msgs/msg/NavSatFix`
//! - `sensor_msgs/msg/Range`
//! - `sensor_msgs/msg/JointState`
//! - `sensor_msgs/msg/Joy`

pub mod config;
pub mod message;
mod message_impl;
pub mod node;
pub mod subscriber;
pub mod traits;

pub use config::Config;
pub use message::{SynchronizedGroup, TimestampedMessage};
pub use node::MsyncNode;
pub use traits::{HasHeader, Timestamped};
