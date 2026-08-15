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
 * Conflux C++ Library - Type Definitions
 */

#ifndef CONFLUX__TYPES_HPP_
#define CONFLUX__TYPES_HPP_

#include <any>
#include <chrono>
#include <functional>
#include <memory>
#include <string>
#include <unordered_map>
#include <vector>

#include "conflux/visibility.h"

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

#endif  // CONFLUX__TYPES_HPP_
