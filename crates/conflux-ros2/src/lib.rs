//! ROS2 integration utilities for conflux multi-stream synchronization.
//!
//! This crate provides utilities for integrating [conflux-core](../conflux_core/index.html)
//! with ROS2 nodes. It handles the ROS2-specific details like:
//!
//! - Dynamic message subscription (any message type at runtime)
//! - Timestamp extraction from message headers
//! - ROS time to Rust Duration conversion
//!
//! # Overview
//!
//! The main components are:
//!
//! - [`TimestampedMessage`]: A wrapper that holds message data with its timestamp
//! - [`SynchronizedGroup`]: A collection of synchronized messages from multiple topics
//! - [`DynamicSubscriptionHandle`]: A handle to a runtime-created subscription
//! - [`create_dynamic_subscription`]: Create subscriptions for any message type
//!
//! # Example: Embedding Synchronization in Your Node
//!
//! ```ignore
//! use conflux_ros2::{
//!     create_dynamic_subscription, TimestampedMessage, SynchronizedGroup,
//!     ros_time_to_duration,
//! };
//! use conflux_core::{sync, Config};
//! use tokio::sync::mpsc;
//! use futures::stream::{self, StreamExt, TryStreamExt};
//!
//! // Create a channel for messages
//! let (tx, rx) = mpsc::unbounded_channel();
//!
//! // Subscribe to topics
//! let _cam_sub = create_dynamic_subscription(
//!     &node,
//!     "/camera/image",
//!     "sensor_msgs/msg/Image",
//!     qos,
//!     tx.clone(),
//! )?;
//!
//! let _lidar_sub = create_dynamic_subscription(
//!     &node,
//!     "/lidar/points",
//!     "sensor_msgs/msg/PointCloud2",
//!     qos,
//!     tx,
//! )?;
//!
//! // Convert receiver to stream
//! let input_stream = Box::pin(stream::unfold(rx, |mut rx| async move {
//!     rx.recv().await.map(|msg| (Ok(msg), rx))
//! }));
//!
//! // Configure synchronization
//! let config = Config::basic(
//!     std::time::Duration::from_millis(50),
//!     None,
//!     64,
//! );
//!
//! // Run synchronization
//! let keys = vec!["/camera/image".to_string(), "/lidar/points".to_string()];
//! let (output_stream, _feedback) = sync(input_stream, keys, config)?;
//!
//! // Process synchronized groups
//! output_stream.try_for_each(|group| async move {
//!     let timestamp = group.values().next().map(|m| m.timestamp).unwrap_or_default();
//!     let synced = SynchronizedGroup::new(timestamp, group);
//!
//!     // Process your synchronized messages here
//!     for (topic, msg) in synced.iter() {
//!         println!("Topic: {}, Timestamp: {:?}", topic, msg.timestamp);
//!     }
//!
//!     Ok(())
//! }).await?;
//! ```
//!
//! # Message Type Support
//!
//! This library uses runtime type introspection to support **any** ROS2 message
//! type that has a `std_msgs/Header` field. The message type is specified as a
//! string (e.g., `"sensor_msgs/msg/Image"`), and the corresponding type support
//! library is loaded at runtime.
//!
//! Common supported message types include:
//! - `sensor_msgs/msg/{Image, PointCloud2, Imu, LaserScan, CameraInfo, ...}`
//! - `nav_msgs/msg/{Odometry, Path, OccupancyGrid, ...}`
//! - `geometry_msgs/msg/{PoseStamped, TwistStamped, ...}`
//! - Any custom message type with a `std_msgs/Header` field

pub mod message;
pub mod subscriber;

// Re-export main types
pub use message::{
    duration_to_ros_time, ros_time_to_duration, SynchronizedGroup, TimestampedMessage,
};
pub use subscriber::{
    create_dynamic_subscription, extract_header_stamp, normalize_msg_type,
    DynamicSubscriptionHandle,
};

// Re-export conflux-core for convenience
pub use conflux_core;
