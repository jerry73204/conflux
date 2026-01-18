//! C FFI bindings for conflux synchronization library.
//!
//! This module provides a C-compatible interface to the conflux-core
//! synchronization algorithm for use in C++ ROS2 nodes.

use conflux_core::{DropPolicy as CoreDropPolicy, WithTimestamp, buffer::Buffer, state::State};
use indexmap::IndexMap;
use std::{
    ffi::{CStr, c_char, c_void},
    ptr,
    sync::Arc,
    time::Duration,
};
use tokio::sync::Notify;

/// Opaque handle to a synchronizer instance.
///
/// The synchronizer manages multiple message streams and outputs
/// synchronized groups when messages fall within the configured time window.
pub struct ConfluxSynchronizer {
    state: State<String, FfiMessage>,
    keys: Vec<String>,
}

/// Internal message wrapper that implements WithTimestamp.
#[derive(Clone)]
struct FfiMessage {
    timestamp: Duration,
    user_data: *mut c_void,
}

// Safety: user_data is managed by the C++ side
unsafe impl Send for FfiMessage {}

impl WithTimestamp for FfiMessage {
    fn timestamp(&self) -> Duration {
        self.timestamp
    }
}

/// Policy for handling buffer overflow when pushing new messages.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConfluxDropPolicy {
    /// Reject new messages when buffer is full.
    /// Preserves existing data. Suitable for offline/rosbag processing.
    #[default]
    RejectNew = 0,

    /// Drop the oldest message to make room for the new one.
    /// Always accepts new data. Suitable for realtime processing.
    DropOldest = 1,
}

impl From<ConfluxDropPolicy> for CoreDropPolicy {
    fn from(policy: ConfluxDropPolicy) -> Self {
        match policy {
            ConfluxDropPolicy::RejectNew => CoreDropPolicy::RejectNew,
            ConfluxDropPolicy::DropOldest => CoreDropPolicy::DropOldest,
        }
    }
}

/// Configuration for creating a synchronizer.
#[repr(C)]
pub struct ConfluxConfig {
    /// Time window in milliseconds for grouping messages.
    /// Use 0 for infinite window (no time-based dropping).
    pub window_size_ms: u64,
    /// Maximum number of messages to buffer per stream.
    pub buffer_size: usize,
    /// Policy for handling buffer overflow.
    pub drop_policy: ConfluxDropPolicy,
}

impl Default for ConfluxConfig {
    fn default() -> Self {
        Self {
            window_size_ms: 50,
            buffer_size: 64,
            drop_policy: ConfluxDropPolicy::default(),
        }
    }
}

/// Result codes for FFI operations.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfluxResult {
    /// Operation succeeded.
    Ok = 0,
    /// Invalid argument provided.
    InvalidArgument = 1,
    /// Buffer is full, message rejected.
    BufferFull = 2,
    /// Key not found.
    KeyNotFound = 3,
    /// Null pointer provided.
    NullPointer = 4,
    /// Internal error.
    InternalError = 5,
}

/// Create a new synchronizer with the given configuration and keys.
///
/// # Safety
///
/// - `keys` must be an array of `key_count` valid null-terminated C strings.
/// - Returns a pointer to a new synchronizer instance. The caller is responsible
///   for freeing this with `conflux_synchronizer_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn conflux_synchronizer_new(
    config: *const ConfluxConfig,
    keys: *const *const c_char,
    key_count: usize,
) -> *mut ConfluxSynchronizer {
    unsafe {
        let config = if config.is_null() {
            ConfluxConfig::default()
        } else {
            ptr::read(config)
        };

        if config.buffer_size < 2 {
            return ptr::null_mut();
        }

        if key_count == 0 || keys.is_null() {
            return ptr::null_mut();
        }

        // Parse keys
        let mut key_strings = Vec::with_capacity(key_count);
        for i in 0..key_count {
            let key_ptr = *keys.add(i);
            if key_ptr.is_null() {
                return ptr::null_mut();
            }
            match CStr::from_ptr(key_ptr).to_str() {
                Ok(s) => key_strings.push(s.to_string()),
                Err(_) => return ptr::null_mut(),
            }
        }

        // Create buffers for each key
        let buffers: IndexMap<String, Buffer<FfiMessage>> = key_strings
            .iter()
            .map(|key| (key.clone(), Buffer::with_capacity(config.buffer_size)))
            .collect();

        // Convert window size: 0 means infinite window (None)
        let window_size = if config.window_size_ms == 0 {
            None
        } else {
            Some(Duration::from_millis(config.window_size_ms))
        };

        // Create State directly
        let state = State {
            buffers,
            commit_ts: None,
            buf_size: config.buffer_size,
            window_size,
            drop_policy: config.drop_policy.into(),
            feedback_tx: None,
            staleness_detector: None,
            space_notify: Arc::new(Notify::new()),
        };

        let sync = Box::new(ConfluxSynchronizer {
            state,
            keys: key_strings,
        });

        Box::into_raw(sync)
    }
}

/// Free a synchronizer instance.
///
/// # Safety
///
/// The pointer must have been returned by `conflux_synchronizer_new` and must
/// not be used after this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn conflux_synchronizer_free(sync: *mut ConfluxSynchronizer) {
    unsafe {
        if !sync.is_null() {
            drop(Box::from_raw(sync));
        }
    }
}

/// Push a message to the synchronizer.
///
/// # Safety
///
/// - `sync` must be a valid pointer from `conflux_synchronizer_new`.
/// - `key` must be a valid null-terminated C string for a key provided at creation.
/// - `user_data` is an opaque pointer that will be returned in synchronized groups.
///
/// # Returns
///
/// - `ConfluxResult::Ok` if the message was accepted.
/// - `ConfluxResult::BufferFull` if the buffer for this key is full.
/// - `ConfluxResult::KeyNotFound` if the key was not provided at creation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn conflux_push_message(
    sync: *mut ConfluxSynchronizer,
    key: *const c_char,
    timestamp_ns: i64,
    user_data: *mut c_void,
) -> ConfluxResult {
    unsafe {
        if sync.is_null() || key.is_null() {
            return ConfluxResult::NullPointer;
        }

        let sync = &mut *sync;
        let key_str = match CStr::from_ptr(key).to_str() {
            Ok(s) => s.to_string(),
            Err(_) => return ConfluxResult::InvalidArgument,
        };

        if !sync.keys.contains(&key_str) {
            return ConfluxResult::KeyNotFound;
        }

        let timestamp = if timestamp_ns >= 0 {
            Duration::from_nanos(timestamp_ns as u64)
        } else {
            return ConfluxResult::InvalidArgument;
        };

        let message = FfiMessage {
            timestamp,
            user_data,
        };

        match sync.state.push(key_str, message) {
            Ok(()) => ConfluxResult::Ok,
            Err(_) => ConfluxResult::BufferFull,
        }
    }
}

/// Poll for a synchronized group of messages.
///
/// This function checks if there's a complete synchronized group available
/// and returns it via the callback.
///
/// # Safety
///
/// - `sync` must be a valid pointer from `conflux_synchronizer_new`.
/// - `callback` will be called with each key-value pair in the synchronized group.
/// - `context` is passed through to the callback.
///
/// # Callback
///
/// The callback receives:
/// - `key`: The topic/key name (null-terminated string)
/// - `timestamp_ns`: Message timestamp in nanoseconds
/// - `user_data`: The user data pointer passed to `conflux_push_message`
/// - `context`: The context pointer passed to this function
///
/// # Returns
///
/// - 1 if a synchronized group was found and callback was invoked.
/// - 0 if no synchronized group is available.
/// - -1 on error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn conflux_poll(
    sync: *mut ConfluxSynchronizer,
    callback: Option<
        extern "C" fn(
            key: *const c_char,
            timestamp_ns: i64,
            user_data: *mut c_void,
            context: *mut c_void,
        ),
    >,
    context: *mut c_void,
) -> i32 {
    unsafe {
        if sync.is_null() {
            return -1;
        }

        let sync = &mut *sync;

        match sync.state.try_match() {
            Some(group) => {
                if let Some(cb) = callback {
                    for (key, msg) in group {
                        let key_cstr = std::ffi::CString::new(key).unwrap();
                        let timestamp_ns = msg.timestamp.as_nanos() as i64;
                        cb(key_cstr.as_ptr(), timestamp_ns, msg.user_data, context);
                    }
                }
                1
            }
            None => 0,
        }
    }
}

/// Get the number of keys registered with the synchronizer.
///
/// # Safety
///
/// `sync` must be a valid pointer from `conflux_synchronizer_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn conflux_key_count(sync: *const ConfluxSynchronizer) -> usize {
    unsafe {
        if sync.is_null() {
            return 0;
        }
        (*sync).keys.len()
    }
}

/// Check if the synchronizer is ready (all buffers have at least 2 messages).
///
/// # Safety
///
/// `sync` must be a valid pointer from `conflux_synchronizer_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn conflux_is_ready(sync: *const ConfluxSynchronizer) -> bool {
    unsafe {
        if sync.is_null() {
            return false;
        }
        (*sync).state.is_ready()
    }
}

/// Check if the synchronizer is empty (any buffer is empty).
///
/// # Safety
///
/// `sync` must be a valid pointer from `conflux_synchronizer_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn conflux_is_empty(sync: *const ConfluxSynchronizer) -> bool {
    unsafe {
        if sync.is_null() {
            return true;
        }
        (*sync).state.is_empty()
    }
}

/// Get the buffer size for a specific key.
///
/// # Safety
///
/// - `sync` must be a valid pointer from `conflux_synchronizer_new`.
/// - `key` must be a valid null-terminated C string.
///
/// # Returns
///
/// The number of messages in the buffer, or 0 if the key is not found.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn conflux_buffer_len(
    sync: *const ConfluxSynchronizer,
    key: *const c_char,
) -> usize {
    unsafe {
        if sync.is_null() || key.is_null() {
            return 0;
        }

        let sync = &*sync;
        let key_str = match CStr::from_ptr(key).to_str() {
            Ok(s) => s,
            Err(_) => return 0,
        };

        sync.state
            .buffers
            .get(key_str)
            .map(|b| b.len())
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicI32, Ordering};

    #[test]
    fn test_create_and_free() {
        let config = ConfluxConfig {
            window_size_ms: 50,
            buffer_size: 10,
            drop_policy: ConfluxDropPolicy::RejectNew,
        };

        let key1 = std::ffi::CString::new("topic1").unwrap();
        let key2 = std::ffi::CString::new("topic2").unwrap();
        let keys = [key1.as_ptr(), key2.as_ptr()];

        let sync = unsafe { conflux_synchronizer_new(&config, keys.as_ptr(), keys.len()) };
        assert!(!sync.is_null());

        unsafe {
            assert_eq!(conflux_key_count(sync), 2);
            conflux_synchronizer_free(sync);
        }
    }

    static CALLBACK_COUNT: AtomicI32 = AtomicI32::new(0);

    extern "C" fn test_callback(
        _key: *const c_char,
        _timestamp_ns: i64,
        _user_data: *mut c_void,
        _context: *mut c_void,
    ) {
        CALLBACK_COUNT.fetch_add(1, Ordering::SeqCst);
    }

    #[test]
    fn test_push_and_poll() {
        // Reset counter
        CALLBACK_COUNT.store(0, Ordering::SeqCst);

        let config = ConfluxConfig {
            window_size_ms: 100,
            buffer_size: 10,
            drop_policy: ConfluxDropPolicy::RejectNew,
        };

        let key1 = std::ffi::CString::new("topic1").unwrap();
        let key2 = std::ffi::CString::new("topic2").unwrap();
        let keys = [key1.as_ptr(), key2.as_ptr()];

        let sync = unsafe { conflux_synchronizer_new(&config, keys.as_ptr(), keys.len()) };
        assert!(!sync.is_null());

        unsafe {
            // Push messages with close timestamps
            // Using integer values as opaque user_data identifiers for testing
            #[allow(clippy::manual_dangling_ptr)]
            let user_data1 = 1usize as *mut c_void;
            #[allow(clippy::manual_dangling_ptr)]
            let user_data2 = 2usize as *mut c_void;

            let result = conflux_push_message(sync, key1.as_ptr(), 1_000_000_000, user_data1);
            assert_eq!(result, ConfluxResult::Ok);

            let result = conflux_push_message(sync, key2.as_ptr(), 1_000_000_000, user_data2);
            assert_eq!(result, ConfluxResult::Ok);

            // Push more messages to enable matching
            let result = conflux_push_message(sync, key1.as_ptr(), 1_100_000_000, user_data1);
            assert_eq!(result, ConfluxResult::Ok);

            let result = conflux_push_message(sync, key2.as_ptr(), 1_100_000_000, user_data2);
            assert_eq!(result, ConfluxResult::Ok);

            // Poll should find a match
            let result = conflux_poll(sync, Some(test_callback), ptr::null_mut());
            assert_eq!(result, 1);
            assert_eq!(CALLBACK_COUNT.load(Ordering::SeqCst), 2); // Two keys in the group

            conflux_synchronizer_free(sync);
        }
    }

    #[test]
    fn test_invalid_key() {
        let config = ConfluxConfig {
            window_size_ms: 50,
            buffer_size: 10,
            drop_policy: ConfluxDropPolicy::RejectNew,
        };

        let key1 = std::ffi::CString::new("topic1").unwrap();
        let keys = [key1.as_ptr()];

        let sync = unsafe { conflux_synchronizer_new(&config, keys.as_ptr(), keys.len()) };
        assert!(!sync.is_null());

        unsafe {
            let invalid_key = std::ffi::CString::new("unknown").unwrap();
            let result =
                conflux_push_message(sync, invalid_key.as_ptr(), 1_000_000_000, ptr::null_mut());
            assert_eq!(result, ConfluxResult::KeyNotFound);

            conflux_synchronizer_free(sync);
        }
    }
}
