/*
 * Conflux C++ Library - Synchronizer
 *
 * Multi-stream message synchronization for ROS2 C++ nodes.
 *
 * License: MIT OR Apache-2.0
 */

#ifndef CONFLUX_SYNCHRONIZER_HPP
#define CONFLUX_SYNCHRONIZER_HPP

#include "conflux/types.hpp"
#include "conflux/visibility.h"

#include "rclcpp/rclcpp.hpp"

#include <chrono>
#include <functional>
#include <memory>
#include <string>
#include <type_traits>
#include <vector>

namespace conflux {

/// Multi-stream message synchronizer for ROS2.
///
/// This class provides a high-level interface for synchronizing messages
/// from multiple ROS2 topics. Messages within the configured time window
/// are grouped together and delivered via a callback.
///
/// Example usage:
/// ```cpp
/// #include <conflux/synchronizer.hpp>
/// #include <sensor_msgs/msg/image.hpp>
/// #include <sensor_msgs/msg/point_cloud2.hpp>
///
/// auto node = std::make_shared<rclcpp::Node>("sync_node");
///
/// conflux::Config config;
/// config.window_size = std::chrono::milliseconds(50);
///
/// conflux::Synchronizer sync(config);
///
/// sync.add_subscription<sensor_msgs::msg::Image>(node, "/camera/image");
/// sync.add_subscription<sensor_msgs::msg::PointCloud2>(node, "/lidar/points");
///
/// sync.on_synchronized([](const conflux::SyncGroup& group) {
///     auto image = group.get<sensor_msgs::msg::Image>("/camera/image");
///     auto points = group.get<sensor_msgs::msg::PointCloud2>("/lidar/points");
///     // Process synchronized messages
/// });
/// ```
class CONFLUX_EXPORT Synchronizer {
public:
    /// Create a new synchronizer with the given configuration.
    explicit Synchronizer(const Config& config);

    /// Destructor.
    ~Synchronizer();

    // Non-copyable
    Synchronizer(const Synchronizer&) = delete;
    Synchronizer& operator=(const Synchronizer&) = delete;

    // Movable
    Synchronizer(Synchronizer&&) noexcept;
    Synchronizer& operator=(Synchronizer&&) noexcept;

    /// Add a topic to synchronize.
    ///
    /// Creates a subscription for the given topic and registers it with
    /// the synchronizer. The message type must have a `header` field with
    /// a `stamp` member for timestamp extraction.
    ///
    /// @tparam MsgT The ROS2 message type
    /// @param node The ROS2 node to create the subscription on
    /// @param topic The topic name to subscribe to
    /// @param qos QoS profile for the subscription (default: SensorDataQoS)
    template <typename MsgT>
    void add_subscription(rclcpp::Node::SharedPtr node, const std::string& topic,
                          const rclcpp::QoS& qos = rclcpp::SensorDataQoS()) {
        // Store topic for later initialization
        add_topic(topic);

        // Create subscription
        auto callback = [this, topic](typename MsgT::SharedPtr msg) {
            // Extract timestamp from header
            int64_t timestamp_ns = static_cast<int64_t>(msg->header.stamp.sec) * 1000000000LL +
                                   static_cast<int64_t>(msg->header.stamp.nanosec);

            // Store message and push to synchronizer
            push_message(topic, timestamp_ns, std::any(std::move(*msg)));
        };

        auto sub = node->create_subscription<MsgT>(topic, qos, callback);
        subscriptions_.push_back(sub);
    }

    /// Register a callback for synchronized message groups.
    ///
    /// The callback will be invoked each time a synchronized group is
    /// available. This also finalizes the synchronizer - no more topics
    /// can be added after this call.
    ///
    /// @param callback Function to call with synchronized message groups
    void on_synchronized(SyncCallback callback);

    /// Process pending messages and invoke callbacks.
    ///
    /// Call this method periodically (e.g., from a timer) to check for
    /// synchronized groups and invoke the registered callback.
    void spin_once();

    /// Get the number of registered topics.
    size_t topic_count() const;

    /// Check if the synchronizer is ready (all buffers have messages).
    bool is_ready() const;

private:
    /// Add a topic to the synchronizer (before finalization).
    void add_topic(const std::string& topic);

    /// Finalize the synchronizer (called by on_synchronized).
    void finalize();

    /// Push a message to the synchronizer.
    void push_message(const std::string& topic, int64_t timestamp_ns, std::any message);

    class Impl;
    std::unique_ptr<Impl> impl_;
    std::vector<rclcpp::SubscriptionBase::SharedPtr> subscriptions_;
};

}  // namespace conflux

#endif  // CONFLUX_SYNCHRONIZER_HPP
