//! conflux-node - Multi-stream message synchronization ROS2 node.
//!
//! This crate provides a ROS2 node that synchronizes messages from multiple
//! input topics within configurable time windows using the
//! [conflux-core](../conflux_core/index.html) algorithm.
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
//! # Message Type Support
//!
//! This node uses runtime type introspection to support **any** ROS2 message
//! type that has a `std_msgs/Header` field. The message type is specified as a
//! string in the configuration (e.g., `"sensor_msgs/msg/Image"`), and the
//! corresponding type support library is loaded at runtime.
//!
//! Common supported message types include:
//! - `sensor_msgs/msg/{Image, PointCloud2, Imu, LaserScan, CameraInfo, ...}`
//! - `nav_msgs/msg/{Odometry, Path, OccupancyGrid, ...}`
//! - `geometry_msgs/msg/{PoseStamped, TwistStamped, ...}`
//! - Any custom message type with a header field

pub mod config;
pub mod node;

pub use config::Config;
pub use node::ConfluxNode;

// Re-export types from conflux-ros2 for convenience
pub use conflux_ros2::{
    DynamicSubscriptionHandle, SynchronizedGroup, TimestampedMessage, create_dynamic_subscription,
};
