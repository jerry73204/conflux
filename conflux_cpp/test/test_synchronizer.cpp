// L-22: conflux_cpp builds libconflux_ffi.so -- the library every LCTK solver
// node loads -- and had no test of any kind, behind a `just test-cpp` recipe
// that echoed two lines and exited 0.
//
// The wrapper's push path is private and reachable only through ROS
// subscriptions, so these drive it the way production does: a real node, real
// publishers, real spinning. The matching algorithm itself is covered at the
// FFI and core layers (conflux-ffi's tests, conflux-core's suite); what is
// tested here is the C++ wrapper's own bookkeeping and dispatch.

#include "conflux/synchronizer.hpp"
#include "conflux/types.hpp"

#include <geometry_msgs/msg/point_stamped.hpp>
#include <gtest/gtest.h>
#include <rclcpp/rclcpp.hpp>

#include <chrono>
#include <memory>
#include <string>
#include <vector>

namespace {

using namespace std::chrono_literals;

conflux::Config MakeConfig(int64_t window_ms, size_t buffer_size) {
    conflux::Config config;
    config.window_size = std::chrono::milliseconds(window_ms);
    config.buffer_size = buffer_size;
    return config;
}

// The wrapper reads `msg->header.stamp`, so the message type must *contain* a
// header rather than be one.
geometry_msgs::msg::PointStamped Stamped(int32_t sec, uint32_t nanosec) {
    geometry_msgs::msg::PointStamped msg;
    msg.header.stamp.sec = sec;
    msg.header.stamp.nanosec = nanosec;
    return msg;
}

class SynchronizerTest : public ::testing::Test {
protected:
    void SetUp() override { node_ = std::make_shared<rclcpp::Node>("conflux_cpp_test_node"); }

    void TearDown() override { node_.reset(); }

    // Spin until `predicate` holds or the budget runs out. Returns whether it held.
    bool SpinUntil(conflux::Synchronizer& sync, const std::function<bool()>& predicate,
                   std::chrono::milliseconds budget = 2s) {
        const auto deadline = std::chrono::steady_clock::now() + budget;
        while (std::chrono::steady_clock::now() < deadline) {
            rclcpp::spin_some(node_);
            sync.spin_once();
            if (predicate()) {
                return true;
            }
            std::this_thread::sleep_for(5ms);
        }
        return predicate();
    }

    rclcpp::Node::SharedPtr node_;
};

TEST_F(SynchronizerTest, TopicCountTracksAddedSubscriptions) {
    conflux::Synchronizer sync(MakeConfig(50, 8));
    EXPECT_EQ(sync.topic_count(), 0u);

    sync.add_subscription<geometry_msgs::msg::PointStamped>(node_, "/a");
    EXPECT_EQ(sync.topic_count(), 1u);

    sync.add_subscription<geometry_msgs::msg::PointStamped>(node_, "/b");
    EXPECT_EQ(sync.topic_count(), 2u);
}

TEST_F(SynchronizerTest, NotReadyBeforeAnyMessageArrives) {
    conflux::Synchronizer sync(MakeConfig(50, 8));
    sync.add_subscription<geometry_msgs::msg::PointStamped>(node_, "/a");
    sync.add_subscription<geometry_msgs::msg::PointStamped>(node_, "/b");
    sync.on_synchronized([](const conflux::SyncGroup&) {});

    EXPECT_FALSE(sync.is_ready());
}

TEST_F(SynchronizerTest, SpinOnceIsSafeWithNoCallbackRegistered) {
    conflux::Synchronizer sync(MakeConfig(50, 8));
    sync.add_subscription<geometry_msgs::msg::PointStamped>(node_, "/a");

    // Must not crash or throw when nothing has been registered or received.
    EXPECT_NO_THROW(sync.spin_once());
}

TEST_F(SynchronizerTest, DeliversAlignedMessagesToTheCallback) {
    conflux::Synchronizer sync(MakeConfig(100, 8));
    sync.add_subscription<geometry_msgs::msg::PointStamped>(node_, "/a");
    sync.add_subscription<geometry_msgs::msg::PointStamped>(node_, "/b");

    size_t groups = 0;
    std::vector<std::string> topics_seen;
    sync.on_synchronized([&](const conflux::SyncGroup& group) {
        ++groups;
        if (topics_seen.empty()) {
            topics_seen = group.topics();
        }
    });

    auto pub_a = node_->create_publisher<geometry_msgs::msg::PointStamped>("/a", 10);
    auto pub_b = node_->create_publisher<geometry_msgs::msg::PointStamped>("/b", 10);

    // Give discovery a moment to connect the local pub/sub pairs.
    SpinUntil(sync, [&] {
        return pub_a->get_subscription_count() > 0 && pub_b->get_subscription_count() > 0;
    });

    for (int i = 0; i < 4; ++i) {
        pub_a->publish(Stamped(100 + i, 0));
        pub_b->publish(Stamped(100 + i, 5000000));  // +5ms
    }

    ASSERT_TRUE(SpinUntil(sync, [&] { return groups > 0; }))
        << "no synchronized group was delivered to the callback";
    EXPECT_EQ(topics_seen.size(), 2u) << "a group should carry both topics";
}

TEST_F(SynchronizerTest, DestructionWithPendingMessagesIsClean) {
    auto pub = node_->create_publisher<geometry_msgs::msg::PointStamped>("/a", 10);
    {
        conflux::Synchronizer sync(MakeConfig(50, 8));
        sync.add_subscription<geometry_msgs::msg::PointStamped>(node_, "/a");
        sync.add_subscription<geometry_msgs::msg::PointStamped>(node_, "/b");
        sync.on_synchronized([](const conflux::SyncGroup&) {});

        SpinUntil(sync, [&] { return pub->get_subscription_count() > 0; });
        pub->publish(Stamped(100, 0));
        SpinUntil(
            sync, [&] { return false; }, 100ms);
        // `sync` is destroyed here holding a buffered message with no partner.
    }
    SUCCEED() << "destruction with a half-filled buffer did not crash";
}

}  // namespace

int main(int argc, char** argv) {
    rclcpp::init(argc, argv);
    ::testing::InitGoogleTest(&argc, argv);
    const int result = RUN_ALL_TESTS();
    rclcpp::shutdown();
    return result;
}
