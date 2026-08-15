//! C FFI bindings for conflux synchronization library.
//!
//! This module provides a C-compatible interface to the conflux-core
//! synchronization algorithm for use in C++ ROS2 nodes.

use conflux_core::{
    BlockedReason, DropPolicy as CoreDropPolicy, WithTimestamp,
    buffer::Buffer,
    state::{PushError, State},
};
use indexmap::IndexMap;
use std::{
    ffi::{CStr, c_char, c_void},
    ptr,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::Notify;

/// Opaque handle to a synchronizer instance.
///
/// The synchronizer manages multiple message streams and outputs
/// synchronized groups when messages fall within the configured time window.
pub struct ConfluxSynchronizer {
    // M-08: the Python binding may drive push/poll from different threads under a
    // MultiThreadedExecutor while ctypes has released the GIL. Guard the mutable
    // core State with a Mutex so concurrent FFI calls are serialized instead of
    // aliasing `&mut State` (undefined behavior). `keys` is immutable after
    // construction and needs no lock.
    state: Mutex<State<String, FfiMessage>>,
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
    /// Message rejected: timestamp is before the commit time (arrived too late).
    LateMessage = 6,
    /// Message rejected: timestamp is not monotonically increasing.
    OutOfOrder = 7,
    /// Timed out waiting for buffer space (blocking push only).
    Timeout = 8,
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
            space_notify: Arc::new(Notify::new()),
        };

        let sync = Box::new(ConfluxSynchronizer {
            state: Mutex::new(state),
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

/// Why the matcher is not currently emitting, mirroring `conflux_core::BlockedReason`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfluxBlockedReason {
    /// A group is available right now -- nothing is blocked.
    NotBlocked = 0,
    /// At least one stream has delivered nothing yet.
    WaitingForData = 1,
    /// All streams have data, but everything sits inside a band narrower than
    /// the window, so the matcher is holding out for a better pairing.
    SpreadTooNarrow = 2,
    /// A buffer is at capacity and no group fits the window. Does not resolve on
    /// its own; `conflux_poll` forces progress out of it (C-05).
    BufferFullNoMatch = 3,
}

/// A snapshot of the matcher's own view, for diagnosing why nothing is emitted.
///
/// M-23: timestamps are nanoseconds, and `-1` means "not applicable" (some
/// buffer is empty, or the window is infinite so nothing is being waited for).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ConfluxStatus {
    /// Greatest of the per-stream oldest timestamps, or -1.
    pub inf_ts_ns: i64,
    /// Least of the per-stream newest timestamps, or -1.
    pub sup_ts_ns: i64,
    /// `sup_ts - inf_ts`, or -1.
    pub spread_ns: i64,
    /// Additional spread needed before the matcher stops waiting, or -1.
    pub shortfall_ns: i64,
    /// Why no group is available.
    pub blocked: ConfluxBlockedReason,
}

/// Read the matcher's current status.
///
/// M-23: the input-side counters cannot tell a healthy wait from a stall. This
/// reports what the matcher itself sees, so a caller can answer "why is it not
/// matching?" without attaching a debugger.
///
/// # Safety
///
/// - `sync` must be a valid pointer from `conflux_synchronizer_new`.
/// - `out` must point to a writable `ConfluxStatus`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn conflux_get_status(
    sync: *const ConfluxSynchronizer,
    out: *mut ConfluxStatus,
) -> ConfluxResult {
    unsafe {
        if sync.is_null() || out.is_null() {
            return ConfluxResult::NullPointer;
        }

        let status = (*sync).state.lock().unwrap().match_status();
        let ns = |d: Option<std::time::Duration>| d.map_or(-1, |d| d.as_nanos() as i64);

        ptr::write(
            out,
            ConfluxStatus {
                inf_ts_ns: ns(status.inf_ts),
                sup_ts_ns: ns(status.sup_ts),
                spread_ns: ns(status.spread),
                shortfall_ns: ns(status.shortfall),
                blocked: match status.blocked {
                    None => ConfluxBlockedReason::NotBlocked,
                    Some(BlockedReason::WaitingForData) => ConfluxBlockedReason::WaitingForData,
                    Some(BlockedReason::SpreadTooNarrow) => ConfluxBlockedReason::SpreadTooNarrow,
                    Some(BlockedReason::BufferFullNoMatch) => {
                        ConfluxBlockedReason::BufferFullNoMatch
                    }
                },
            },
        );
        ConfluxResult::Ok
    }
}

/// Discard all buffered messages and forget all timestamp history.
///
/// M-22: call this when the message source restarts its clock -- a rosbag loop,
/// a sim-time reset, or a sensor that reconnects and restarts its stamp counter.
/// Without it the affected buffer rejects every later message as out-of-order
/// forever, and because a group needs all streams non-empty, one dead stream
/// stalls the entire synchronizer with no way back.
///
/// Buffered messages are dropped. Callers holding references keyed by
/// `user_data` should reconcile with `conflux_for_each_live` after this call --
/// which will report nothing live, since every buffer is now empty.
///
/// # Safety
///
/// `sync` must be a valid pointer from `conflux_synchronizer_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn conflux_synchronizer_reset(sync: *mut ConfluxSynchronizer) {
    unsafe {
        if sync.is_null() {
            return;
        }
        (*sync).state.lock().unwrap().reset();
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

        let sync = &*sync;
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

        // H-05: map each push failure to a distinct result code instead of
        // collapsing them all to BufferFull. Late and out-of-order rejections
        // are normal under BEST_EFFORT and must not be counted as buffer
        // overflows, otherwise the rejection statistics and overflow warnings
        // are inflated (even DropOldest, which never really overflows).
        match sync.state.lock().unwrap().push(key_str, message) {
            Ok(()) => ConfluxResult::Ok,
            Err(PushError::BufferFull(_)) => ConfluxResult::BufferFull,
            Err(PushError::LateMessage(_)) => ConfluxResult::LateMessage,
            Err(PushError::OutOfOrder(_)) => ConfluxResult::OutOfOrder,
            Err(PushError::Timeout(_)) => ConfluxResult::Timeout,
            Err(PushError::UnknownKey(_)) => ConfluxResult::KeyNotFound,
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

        let sync = &*sync;

        // Take the matched group under the lock, then release it before invoking
        // the (Python) callback, so the callback can never re-enter and deadlock.
        //
        // C-05/H-12: go through `State::advance`, not `try_match`. `advance` owns
        // the shared "match, or force progress when full and unmatchable" rule, so
        // the FFI can no longer wedge where the pure-Rust pipeline recovers.
        let group_opt = sync.state.lock().unwrap().advance();
        match group_opt {
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

/// Invoke `callback` once for every message currently held in a buffer, passing
/// its `user_data`.
///
/// C-02: DropOldest eviction and finite-window pruning discard messages silently
/// (the push still returns Ok), so a caller that keeps a table of message
/// references keyed by `user_data` (e.g. the Python binding) never learns they
/// were dropped and leaks one reference per evicted message. This lets the caller
/// reconcile its table against the set of still-live messages and free the rest.
///
/// # Safety
///
/// `sync` must be a valid pointer from `conflux_synchronizer_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn conflux_for_each_live(
    sync: *const ConfluxSynchronizer,
    callback: Option<extern "C" fn(user_data: *mut c_void, context: *mut c_void)>,
    context: *mut c_void,
) {
    unsafe {
        if sync.is_null() {
            return;
        }
        let sync = &*sync;
        // Collect the live user_data under the lock, then release it before
        // invoking the callback.
        let ptrs: Vec<*mut c_void> = {
            let state = sync.state.lock().unwrap();
            state
                .buffers
                .values()
                .flat_map(|buffer| buffer.iter().map(|msg| msg.user_data))
                .collect()
        };
        if let Some(cb) = callback {
            for ptr in ptrs {
                cb(ptr, context);
            }
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
        (*sync).state.lock().unwrap().is_ready()
    }
}

/// Check if the synchronizer is empty (any buffer is empty).
///
/// # Safety
///
/// `sync` must be a valid pointer from `conflux_synchronizer_new`.
///
/// L-17: prefer `conflux_has_empty_buffer`. The name `is_empty` reads as "the
/// synchronizer holds nothing", but it reports whether ANY buffer is empty.
/// Retained so existing callers keep linking.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn conflux_is_empty(sync: *const ConfluxSynchronizer) -> bool {
    unsafe {
        if sync.is_null() {
            return true;
        }
        (*sync).state.lock().unwrap().has_empty_buffer()
    }
}

/// Returns true when **at least one** buffer is empty, i.e. no group can form.
///
/// # Safety
///
/// `sync` must be a valid pointer from `conflux_synchronizer_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn conflux_has_empty_buffer(sync: *const ConfluxSynchronizer) -> bool {
    unsafe {
        if sync.is_null() {
            return false;
        }
        (*sync).state.lock().unwrap().has_empty_buffer()
    }
}

/// Returns true when **every** buffer is empty -- the synchronizer is idle.
///
/// # Safety
///
/// `sync` must be a valid pointer from `conflux_synchronizer_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn conflux_all_buffers_empty(sync: *const ConfluxSynchronizer) -> bool {
    unsafe {
        if sync.is_null() {
            return true;
        }
        (*sync).state.lock().unwrap().all_buffers_empty()
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
            .lock()
            .unwrap()
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

    /// C-05: after the two streams diverge far enough that no group can be formed,
    /// the buffers fill and `try_match` refuses to emit (spread < window, and
    /// `all_one()` is false). The pure-Rust `sync()` pipeline escapes this via
    /// `is_full -> drop_min`; the FFI must make the same forced progress, or the
    /// synchronizer is wedged for the life of the process.
    fn wedge_scenario(policy: ConfluxDropPolicy) -> (i32, i32) {
        const MS: i64 = 1_000_000;

        let config = ConfluxConfig {
            window_size_ms: 50,
            buffer_size: 2,
            drop_policy: policy,
        };

        let key_a = std::ffi::CString::new("A").unwrap();
        let key_b = std::ffi::CString::new("B").unwrap();
        let keys = [key_a.as_ptr(), key_b.as_ptr()];

        let sync = unsafe { conflux_synchronizer_new(&config, keys.as_ptr(), keys.len()) };
        assert!(!sync.is_null());

        let mut accepted = 0;
        let mut groups = 0;

        unsafe {
            // Diverge the streams: no pair falls inside a common 50 ms window.
            for ts in [1000, 1010] {
                conflux_push_message(sync, key_a.as_ptr(), ts * MS, ptr::null_mut());
            }
            for ts in [5000, 5010] {
                conflux_push_message(sync, key_b.as_ptr(), ts * MS, ptr::null_mut());
            }

            // Now feed perfectly aligned fresh data and drain after every pair.
            for i in 0..20i64 {
                let t = 6000 + i * 33;
                if conflux_push_message(sync, key_a.as_ptr(), t * MS, ptr::null_mut())
                    == ConfluxResult::Ok
                {
                    accepted += 1;
                }
                if conflux_push_message(sync, key_b.as_ptr(), (t + 5) * MS, ptr::null_mut())
                    == ConfluxResult::Ok
                {
                    accepted += 1;
                }
                while conflux_poll(sync, None, ptr::null_mut()) == 1 {
                    groups += 1;
                }
            }

            conflux_synchronizer_free(sync);
        }

        (accepted, groups)
    }

    #[test]
    fn test_recovers_from_divergence_reject_new() {
        let (accepted, groups) = wedge_scenario(ConfluxDropPolicy::RejectNew);
        assert!(
            accepted > 0,
            "RejectNew wedged: 0/40 fresh aligned pushes accepted after a divergence"
        );
        assert!(
            groups > 0,
            "RejectNew wedged: no group emitted from 20 aligned pairs after a divergence"
        );
    }

    #[test]
    fn test_recovers_from_divergence_drop_oldest() {
        let (accepted, groups) = wedge_scenario(ConfluxDropPolicy::DropOldest);
        assert_eq!(accepted, 40, "DropOldest should accept every fresh message");
        assert!(
            groups > 0,
            "DropOldest wedged: all 40 pushes accepted but no group ever emitted"
        );
    }

    /// M-22: a clock restart (bag loop, sim-time reset, sensor reconnect) must be
    /// recoverable through the C ABI, not just from Rust.
    #[test]
    fn test_reset_revives_stream_after_clock_jump() {
        const MS: i64 = 1_000_000;

        let config = ConfluxConfig {
            window_size_ms: 50,
            buffer_size: 8,
            drop_policy: ConfluxDropPolicy::RejectNew,
        };

        let key_a = std::ffi::CString::new("A").unwrap();
        let key_b = std::ffi::CString::new("B").unwrap();
        let keys = [key_a.as_ptr(), key_b.as_ptr()];

        let sync = unsafe { conflux_synchronizer_new(&config, keys.as_ptr(), keys.len()) };
        assert!(!sync.is_null());

        unsafe {
            // Normal operation, and a group so the commit timestamp advances.
            conflux_push_message(sync, key_a.as_ptr(), 5000 * MS, ptr::null_mut());
            conflux_push_message(sync, key_b.as_ptr(), 5010 * MS, ptr::null_mut());
            assert_eq!(conflux_poll(sync, None, ptr::null_mut()), 1);

            // The source restarts its clock: every push is now refused.
            assert_ne!(
                conflux_push_message(sync, key_a.as_ptr(), 1000 * MS, ptr::null_mut()),
                ConfluxResult::Ok
            );

            conflux_synchronizer_reset(sync);

            assert_eq!(
                conflux_push_message(sync, key_a.as_ptr(), 1000 * MS, ptr::null_mut()),
                ConfluxResult::Ok
            );
            assert_eq!(
                conflux_push_message(sync, key_b.as_ptr(), 1010 * MS, ptr::null_mut()),
                ConfluxResult::Ok
            );
            assert_eq!(
                conflux_poll(sync, None, ptr::null_mut()),
                1,
                "synchronizer should emit again after a reset"
            );

            conflux_synchronizer_free(sync);
        }
    }

    /// M-23: the wedge state must be reportable through the C ABI.
    #[test]
    fn test_status_reports_blocked_reason() {
        const MS: i64 = 1_000_000;

        let config = ConfluxConfig {
            window_size_ms: 50,
            buffer_size: 2,
            drop_policy: ConfluxDropPolicy::RejectNew,
        };

        let key_a = std::ffi::CString::new("A").unwrap();
        let key_b = std::ffi::CString::new("B").unwrap();
        let keys = [key_a.as_ptr(), key_b.as_ptr()];

        let sync = unsafe { conflux_synchronizer_new(&config, keys.as_ptr(), keys.len()) };
        assert!(!sync.is_null());

        unsafe {
            let mut status = ConfluxStatus {
                inf_ts_ns: 0,
                sup_ts_ns: 0,
                spread_ns: 0,
                shortfall_ns: 0,
                blocked: ConfluxBlockedReason::NotBlocked,
            };

            // Nothing pushed yet.
            assert_eq!(conflux_get_status(sync, &mut status), ConfluxResult::Ok);
            assert_eq!(status.blocked, ConfluxBlockedReason::WaitingForData);
            assert_eq!(status.inf_ts_ns, -1);

            // Only one stream has data.
            conflux_push_message(sync, key_a.as_ptr(), 1000 * MS, ptr::null_mut());
            conflux_get_status(sync, &mut status);
            assert_eq!(status.blocked, ConfluxBlockedReason::WaitingForData);

            // The C-05 shape: buffers full, nothing pairs.
            conflux_push_message(sync, key_a.as_ptr(), 1010 * MS, ptr::null_mut());
            conflux_push_message(sync, key_b.as_ptr(), 5000 * MS, ptr::null_mut());
            conflux_push_message(sync, key_b.as_ptr(), 5010 * MS, ptr::null_mut());
            conflux_get_status(sync, &mut status);
            assert_eq!(status.blocked, ConfluxBlockedReason::BufferFullNoMatch);
            assert!(status.shortfall_ns > 0, "a shortfall should be reported");

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
