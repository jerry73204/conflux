/*
 * Conflux C++ Library - FFI Bridge Header
 *
 * Internal header for C++ wrappers around the C FFI functions.
 *
 * License: MIT OR Apache-2.0
 */

#ifndef CONFLUX_FFI_BRIDGE_HPP
#define CONFLUX_FFI_BRIDGE_HPP

#include <cstdint>
#include <functional>
#include <string>
#include <vector>

// Forward declare the opaque Rust type
struct ConfluxSynchronizer;

namespace conflux {
namespace ffi {

/// Opaque handle to the Rust synchronizer.
struct SynchronizerHandle {
    ConfluxSynchronizer* ptr = nullptr;
};

/// Result codes for push operations.
enum class PushResult { Ok, InvalidArgument, BufferFull, KeyNotFound, NullPointer, InternalError };

/// Callback type for poll results.
using PollCallback = void (*)(const char* key, int64_t timestamp_ns, void* user_data,
                              void* context);

/// Create a new synchronizer.
SynchronizerHandle create_synchronizer(uint64_t window_size_ms, size_t buffer_size,
                                       const std::vector<std::string>& topics);

/// Destroy a synchronizer.
void destroy_synchronizer(SynchronizerHandle handle);

/// Push a message to the synchronizer.
PushResult push_message(SynchronizerHandle handle, const std::string& topic, int64_t timestamp_ns,
                        void* user_data);

/// Poll for synchronized groups.
/// Returns true if a group was found.
bool poll(SynchronizerHandle handle, PollCallback callback, void* context);

/// Get the number of registered topics.
size_t key_count(SynchronizerHandle handle);

/// Check if all buffers have at least 2 messages.
bool is_ready(SynchronizerHandle handle);

/// Check if any buffer is empty.
bool is_empty(SynchronizerHandle handle);

/// Get the buffer length for a specific topic.
size_t buffer_len(SynchronizerHandle handle, const std::string& topic);

}  // namespace ffi
}  // namespace conflux

#endif  // CONFLUX_FFI_BRIDGE_HPP
