/*
 * Conflux C++ Library - Synchronizer Implementation
 *
 * License: MIT OR Apache-2.0
 */

#include "conflux/synchronizer.hpp"

#include "ffi_bridge.hpp"

#include <mutex>
#include <unordered_map>

namespace conflux {

/// Internal implementation of the Synchronizer class.
class Synchronizer::Impl {
public:
    Impl(const Config& config) : config_(config), finalized_(false), handle_{nullptr} {}

    ~Impl() {
        if (handle_.ptr) {
            ffi::destroy_synchronizer(handle_);
        }
    }

    void add_topic(const std::string& topic) {
        if (finalized_) {
            throw std::runtime_error("Cannot add topics after on_synchronized() is called");
        }
        topics_.push_back(topic);
    }

    void finalize() {
        if (finalized_) {
            return;
        }

        handle_ =
            ffi::create_synchronizer(config_.window_size.count(), config_.buffer_size, topics_);

        if (!handle_.ptr) {
            throw std::runtime_error("Failed to create synchronizer");
        }

        finalized_ = true;
    }

    void set_callback(SyncCallback callback) { callback_ = std::move(callback); }

    void push_message(const std::string& topic, int64_t timestamp_ns, std::any message) {
        if (!finalized_) {
            // Lazily finalize on first message
            finalize();
        }

        // Store the message
        size_t msg_id;
        {
            std::lock_guard<std::mutex> lock(mutex_);
            msg_id = next_msg_id_++;
            pending_messages_[msg_id] =
                PendingMessage{topic, std::chrono::nanoseconds(timestamp_ns), std::move(message)};
        }

        // Push to the Rust synchronizer with the message ID as user_data
        auto result =
            ffi::push_message(handle_, topic, timestamp_ns, reinterpret_cast<void*>(msg_id));

        if (result != ffi::PushResult::Ok) {
            // Remove the pending message on failure
            std::lock_guard<std::mutex> lock(mutex_);
            pending_messages_.erase(msg_id);
        }
    }

    void spin_once() {
        if (!finalized_ || !callback_) {
            return;
        }

        // Keep polling until no more groups
        while (true) {
            SyncGroup group;
            bool found = false;

            // Collect messages from the callback
            auto callback = [](const char* key, int64_t timestamp_ns, void* user_data,
                               void* context) {
                auto* impl = static_cast<Impl*>(context);
                size_t msg_id = reinterpret_cast<size_t>(user_data);

                std::lock_guard<std::mutex> lock(impl->mutex_);
                auto it = impl->pending_messages_.find(msg_id);
                if (it != impl->pending_messages_.end()) {
                    impl->current_group_->timestamp_ = std::chrono::nanoseconds(timestamp_ns);
                    impl->current_group_->messages_[key] = std::move(it->second.message);
                    impl->pending_messages_.erase(it);
                }
            };

            {
                std::lock_guard<std::mutex> lock(mutex_);
                current_group_ = &group;
            }

            found = ffi::poll(handle_, callback, this);

            {
                std::lock_guard<std::mutex> lock(mutex_);
                current_group_ = nullptr;
            }

            if (!found) {
                break;
            }

            // Invoke the user callback
            callback_(group);
        }
    }

    size_t topic_count() const { return topics_.size(); }

    bool is_ready() const {
        if (!finalized_) {
            return false;
        }
        return ffi::is_ready(handle_);
    }

private:
    struct PendingMessage {
        std::string topic;
        std::chrono::nanoseconds timestamp;
        std::any message;
    };

    Config config_;
    std::vector<std::string> topics_;
    bool finalized_;
    ffi::SynchronizerHandle handle_;
    SyncCallback callback_;

    std::mutex mutex_;
    size_t next_msg_id_ = 0;
    std::unordered_map<size_t, PendingMessage> pending_messages_;
    SyncGroup* current_group_ = nullptr;
};

// Synchronizer implementation

Synchronizer::Synchronizer(const Config& config) : impl_(std::make_unique<Impl>(config)) {}

Synchronizer::~Synchronizer() = default;

Synchronizer::Synchronizer(Synchronizer&&) noexcept = default;
Synchronizer& Synchronizer::operator=(Synchronizer&&) noexcept = default;

void Synchronizer::add_topic(const std::string& topic) {
    impl_->add_topic(topic);
}

void Synchronizer::finalize() {
    impl_->finalize();
}

void Synchronizer::on_synchronized(SyncCallback callback) {
    impl_->finalize();
    impl_->set_callback(std::move(callback));
}

void Synchronizer::push_message(const std::string& topic, int64_t timestamp_ns, std::any message) {
    impl_->push_message(topic, timestamp_ns, std::move(message));
}

void Synchronizer::spin_once() {
    impl_->spin_once();
}

size_t Synchronizer::topic_count() const {
    return impl_->topic_count();
}

bool Synchronizer::is_ready() const {
    return impl_->is_ready();
}

}  // namespace conflux
