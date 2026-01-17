//! HasHeader implementations for common ROS2 message types.
//!
//! This module provides implementations of the `HasHeader` trait for
//! commonly used sensor message types.

use crate::traits::HasHeader;

// =============================================================================
// sensor_msgs implementations
// =============================================================================

impl HasHeader for sensor_msgs::msg::Image {
    fn header_stamp(&self) -> (i32, u32) {
        (self.header.stamp.sec, self.header.stamp.nanosec)
    }

    fn header_frame_id(&self) -> &str {
        &self.header.frame_id
    }
}

impl HasHeader for sensor_msgs::msg::PointCloud2 {
    fn header_stamp(&self) -> (i32, u32) {
        (self.header.stamp.sec, self.header.stamp.nanosec)
    }

    fn header_frame_id(&self) -> &str {
        &self.header.frame_id
    }
}

impl HasHeader for sensor_msgs::msg::Imu {
    fn header_stamp(&self) -> (i32, u32) {
        (self.header.stamp.sec, self.header.stamp.nanosec)
    }

    fn header_frame_id(&self) -> &str {
        &self.header.frame_id
    }
}

impl HasHeader for sensor_msgs::msg::LaserScan {
    fn header_stamp(&self) -> (i32, u32) {
        (self.header.stamp.sec, self.header.stamp.nanosec)
    }

    fn header_frame_id(&self) -> &str {
        &self.header.frame_id
    }
}

impl HasHeader for sensor_msgs::msg::CameraInfo {
    fn header_stamp(&self) -> (i32, u32) {
        (self.header.stamp.sec, self.header.stamp.nanosec)
    }

    fn header_frame_id(&self) -> &str {
        &self.header.frame_id
    }
}

impl HasHeader for sensor_msgs::msg::CompressedImage {
    fn header_stamp(&self) -> (i32, u32) {
        (self.header.stamp.sec, self.header.stamp.nanosec)
    }

    fn header_frame_id(&self) -> &str {
        &self.header.frame_id
    }
}

impl HasHeader for sensor_msgs::msg::NavSatFix {
    fn header_stamp(&self) -> (i32, u32) {
        (self.header.stamp.sec, self.header.stamp.nanosec)
    }

    fn header_frame_id(&self) -> &str {
        &self.header.frame_id
    }
}

impl HasHeader for sensor_msgs::msg::Range {
    fn header_stamp(&self) -> (i32, u32) {
        (self.header.stamp.sec, self.header.stamp.nanosec)
    }

    fn header_frame_id(&self) -> &str {
        &self.header.frame_id
    }
}

impl HasHeader for sensor_msgs::msg::MagneticField {
    fn header_stamp(&self) -> (i32, u32) {
        (self.header.stamp.sec, self.header.stamp.nanosec)
    }

    fn header_frame_id(&self) -> &str {
        &self.header.frame_id
    }
}

impl HasHeader for sensor_msgs::msg::FluidPressure {
    fn header_stamp(&self) -> (i32, u32) {
        (self.header.stamp.sec, self.header.stamp.nanosec)
    }

    fn header_frame_id(&self) -> &str {
        &self.header.frame_id
    }
}

impl HasHeader for sensor_msgs::msg::Temperature {
    fn header_stamp(&self) -> (i32, u32) {
        (self.header.stamp.sec, self.header.stamp.nanosec)
    }

    fn header_frame_id(&self) -> &str {
        &self.header.frame_id
    }
}

impl HasHeader for sensor_msgs::msg::Illuminance {
    fn header_stamp(&self) -> (i32, u32) {
        (self.header.stamp.sec, self.header.stamp.nanosec)
    }

    fn header_frame_id(&self) -> &str {
        &self.header.frame_id
    }
}

impl HasHeader for sensor_msgs::msg::RelativeHumidity {
    fn header_stamp(&self) -> (i32, u32) {
        (self.header.stamp.sec, self.header.stamp.nanosec)
    }

    fn header_frame_id(&self) -> &str {
        &self.header.frame_id
    }
}

impl HasHeader for sensor_msgs::msg::JointState {
    fn header_stamp(&self) -> (i32, u32) {
        (self.header.stamp.sec, self.header.stamp.nanosec)
    }

    fn header_frame_id(&self) -> &str {
        &self.header.frame_id
    }
}

impl HasHeader for sensor_msgs::msg::Joy {
    fn header_stamp(&self) -> (i32, u32) {
        (self.header.stamp.sec, self.header.stamp.nanosec)
    }

    fn header_frame_id(&self) -> &str {
        &self.header.frame_id
    }
}
