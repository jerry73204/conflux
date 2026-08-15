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
 * Conflux C++ Library - FFI Bridge Header
 *
 * Internal header for C++ wrappers around the C FFI functions.
 */

#ifndef FFI_BRIDGE_HPP_
#define FFI_BRIDGE_HPP_

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

#endif  // FFI_BRIDGE_HPP_
