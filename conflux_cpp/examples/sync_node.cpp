// Copyright 2026 jerry73204
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
// This package is dual-licensed: "MIT OR Apache-2.0". The Apache-2.0 notice
// above is reproduced in full because the linter requires a recognised license
// block; it does not narrow the choice. See LICENSE-MIT and LICENSE-APACHE.

/*
 * Example: Multi-sensor synchronization node using conflux_cpp
 *
 * This example demonstrates how to use conflux_cpp to synchronize
 * messages from multiple sensor topics (camera and LiDAR).
 */

#include <memory>

#include "conflux/synchronizer.hpp"
#include "rclcpp/rclcpp.hpp"
#include "sensor_msgs/msg/image.hpp"
#include "sensor_msgs/msg/point_cloud2.hpp"

class SyncProcessorNode : public rclcpp::Node {
public:
    SyncProcessorNode() : Node("sync_processor") {
        // Configure synchronizer with 50ms window
        conflux::Config config;
        config.window_size = std::chrono::milliseconds(50);
        config.buffer_size = 64;

        sync_ = std::make_unique<conflux::Synchronizer>(config);

        // Add topics to synchronize
        sync_->add_subscription<sensor_msgs::msg::Image>(shared_from_this(), "/camera/image");
        sync_->add_subscription<sensor_msgs::msg::PointCloud2>(shared_from_this(), "/lidar/points");

        // Register callback for synchronized groups
        sync_->on_synchronized([this](const conflux::SyncGroup& group) {
            auto image = group.get<sensor_msgs::msg::Image>("/camera/image");
            auto points = group.get<sensor_msgs::msg::PointCloud2>("/lidar/points");

            if (image && points) {
                RCLCPP_INFO(this->get_logger(), "Synchronized: image=%d.%09u, points=%d.%09u",
                            image->header.stamp.sec, image->header.stamp.nanosec,
                            points->header.stamp.sec, points->header.stamp.nanosec);

                process(image, points);
            }
        });

        // Create timer to poll for synchronized groups
        timer_ = create_wall_timer(std::chrono::milliseconds(10), [this]() { sync_->spin_once(); });
    }

private:
    void process(const sensor_msgs::msg::Image* image,
                 const sensor_msgs::msg::PointCloud2* points) {
        // Your processing logic here
        (void)image;
        (void)points;
    }

    std::unique_ptr<conflux::Synchronizer> sync_;
    rclcpp::TimerBase::SharedPtr timer_;
};

int main(int argc, char** argv) {
    rclcpp::init(argc, argv);
    auto node = std::make_shared<SyncProcessorNode>();
    rclcpp::spin(node);
    rclcpp::shutdown();
    return 0;
}
