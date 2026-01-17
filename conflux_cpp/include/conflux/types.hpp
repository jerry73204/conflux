/*
 * Conflux C++ Library - Type Definitions
 *
 * License: MIT OR Apache-2.0
 */

#ifndef CONFLUX_TYPES_HPP
#define CONFLUX_TYPES_HPP

#include "conflux/visibility.h"

#include <any>
#include <chrono>
#include <functional>
#include <memory>
#include <string>
#include <unordered_map>
#include <vector>

namespace conflux {

/// Configuration for the synchronizer.
struct CONFLUX_EXPORT Config {
    /// Time window for grouping messages (default: 50ms).
    std::chrono::milliseconds window_size{50};

    /// Maximum number of messages to buffer per stream (default: 64).
    size_t buffer_size{64};
};

/// A synchronized group of messages from multiple streams.
///
/// Each message is stored with its original type using std::any.
/// Use get<T>() to retrieve messages with type safety.
class CONFLUX_EXPORT SyncGroup {
public:
    /// Get the timestamp of this synchronized group.
    std::chrono::nanoseconds timestamp() const { return timestamp_; }

    /// Get a message by topic name.
    ///
    /// @tparam T The message type (e.g., sensor_msgs::msg::Image)
    /// @param topic The topic name
    /// @return Pointer to the message, or nullptr if not found or wrong type
    template <typename T>
    const T* get(const std::string& topic) const {
        auto it = messages_.find(topic);
        if (it == messages_.end()) {
            return nullptr;
        }
        return std::any_cast<T>(&it->second);
    }

    /// Check if a topic exists in this group.
    bool has(const std::string& topic) const { return messages_.find(topic) != messages_.end(); }

    /// Get all topic names in this group.
    std::vector<std::string> topics() const {
        std::vector<std::string> result;
        result.reserve(messages_.size());
        for (const auto& [topic, _] : messages_) {
            result.push_back(topic);
        }
        return result;
    }

    /// Get the number of messages in this group.
    size_t size() const { return messages_.size(); }

private:
    friend class Synchronizer;

    std::chrono::nanoseconds timestamp_;
    std::unordered_map<std::string, std::any> messages_;
};

/// Callback type for synchronized message groups.
using SyncCallback = std::function<void(const SyncGroup&)>;

}  // namespace conflux

#endif  // CONFLUX_TYPES_HPP
