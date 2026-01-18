use crate::{
    buffer::Buffer,
    config::DropPolicy,
    staleness::StalenessDetector,
    types::{Feedback, Key, WithTimestamp},
};
use indexmap::IndexMap;
use std::{sync::Arc, time::Duration};
use tokio::sync::{Notify, watch};

/// Error type for push operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushError<T> {
    /// Message timestamp is before the commit timestamp (too late).
    LateMessage(T),
    /// Key was not registered.
    UnknownKey(T),
    /// Buffer is full and policy is RejectNew.
    BufferFull(T),
    /// Message timestamp is not monotonically increasing.
    OutOfOrder(T),
    /// Timeout waiting for space in buffer (push_blocking only).
    Timeout(T),
}

impl<T> PushError<T> {
    /// Extract the message from the error.
    pub fn into_inner(self) -> T {
        match self {
            PushError::LateMessage(t) => t,
            PushError::UnknownKey(t) => t,
            PushError::BufferFull(t) => t,
            PushError::OutOfOrder(t) => t,
            PushError::Timeout(t) => t,
        }
    }
}

/// The internal state maintained by [sync](crate::sync).
#[derive(Debug)]
pub struct State<K, T>
where
    K: Key,
    T: WithTimestamp + Clone,
{
    /// A list of buffers indexed by key K.
    pub buffers: IndexMap<K, Buffer<T>>,

    /// Marks the timestamp where messages before the time point are
    /// emitted.
    pub commit_ts: Option<Duration>,

    /// The maximum size of each buffer for each key.
    pub buf_size: usize,

    /// The windows size that a batch of messages should reside
    /// within. None means infinite window.
    pub window_size: Option<Duration>,

    /// Policy for handling buffer overflow.
    pub drop_policy: DropPolicy,

    /// The sender where feedback messages are sent to.
    pub feedback_tx: Option<watch::Sender<Feedback<K>>>,

    /// Optional staleness detector for real-time message expiration
    pub staleness_detector: Option<StalenessDetector<K, T>>,

    /// Notifier for signaling when buffer space becomes available.
    /// Used by push_blocking() to wait for space.
    pub space_notify: Arc<Notify>,
}

impl<K, T> State<K, T>
where
    K: Key,
    T: WithTimestamp + Clone,
{
    // pub fn print_debug_info(&self) {
    //     debug!("buffer sizes");
    //     self.buffers.iter().for_each(|(device, buffer)| {
    //         debug!("- {}:\t{}", device, buffer.buffer.len());
    //     });
    // }

    /// Generate a feedback message.
    pub fn update_feedback(&mut self) {
        let Some(feedback_tx) = &self.feedback_tx else {
            return;
        };

        let accepted_keys: Vec<K> = self
            .buffers
            .iter()
            .filter(|&(_key, buffer)| buffer.len() < self.buf_size)
            .map(|(key, _buffer)| key.clone())
            .collect();

        // Request input sources to deliver messages with ts below thresh_ts
        // let thresh_ts = self
        //     .buffers
        //     .values()
        //     .filter_map(|buffer| buffer.last_ts())
        //     .min();
        // let include_thresh_ts = self.buffers.values().all(|buffer| buffer.buffer.is_empty());

        let msg = Feedback {
            accepted_keys,
            // accepted_max_timestamp: thresh_ts.map(|ts| ts.as_nanos() as u64),
            // inclusive: Some(include_thresh_ts),
            accepted_max_timestamp: None,
            commit_timestamp: self.commit_ts,
        };

        // if self.verbose_debug {
        //     debug!("update feedback with accepted devices:");
        //     accepted_devices.iter().for_each(|device| {
        //         debug!("- {:?}", device);
        //     });
        // }

        if feedback_tx.send(msg).is_err() {
            self.feedback_tx = None;
        }
    }

    /// Try to group up messages within a time window.
    /// If window_size is None (infinite window), messages are matched
    /// without time-based dropping.
    pub fn try_match(&mut self) -> Option<IndexMap<K, T>> {
        let inf_ts = loop {
            let (_, inf_ts) = self.inf_timestamp()?;

            // Checking all buffers have only one data left.
            // Make sure (sup - inf >= window_size). If not, it needs to
            // wait for more messages.
            let (_, sup_ts) = self.sup_timestamp()?;

            // For finite window: check if spread is sufficient
            // For infinite window: skip this check (always ready if all buffers have data)
            if let Some(window_size) = self.window_size {
                if !self.all_one() && inf_ts + window_size > sup_ts {
                    return None;
                }

                let window_start = inf_ts.saturating_sub(window_size);

                // Drop messages before the time window (only for finite window)
                let dropped = self.buffers.values_mut().any(|buffer| {
                    let count = buffer.drop_before(window_start);
                    count > 0
                });

                if !dropped {
                    break inf_ts;
                }
            } else {
                // Infinite window: no time-based checks or dropping
                break inf_ts;
            }
        };

        // Pop one message from each buffer
        let items: IndexMap<_, _> = self
            .buffers
            .iter_mut()
            .map(|(key, buffer)| {
                let item = buffer.pop_front().unwrap();
                // For finite window, verify message is within bounds
                if let Some(window_size) = self.window_size {
                    let window_end = inf_ts.saturating_add(window_size);
                    assert!(item.timestamp() <= window_end);
                }
                (key.clone(), item)
            })
            .collect();

        // update commit timestamp
        let new_commit_ts = items.values().map(|item| item.timestamp()).min().unwrap();
        self.commit_ts = Some(new_commit_ts);

        // Notify waiters that buffer space is now available
        self.space_notify.notify_waiters();

        Some(items)
    }

    /// Gets the minimum of the maximum timestamps from each buffer.
    /// Returns None if any buffer is empty.
    pub fn sup_timestamp(&self) -> Option<(K, Duration)> {
        // First check all buffers have messages
        if self.buffers.values().any(|b| b.is_empty()) {
            return None;
        }
        self.buffers
            .iter()
            .filter_map(|(key, buffer)| {
                // Get the latest timestamp
                let ts = buffer.back()?.timestamp();
                Some((key.clone(), ts))
            })
            .min_by_key(|(_, ts)| *ts)
    }

    /// Gets the maximum of the minimum timestamps from each buffer.
    /// Returns None if any buffer is empty.
    pub fn inf_timestamp(&self) -> Option<(K, Duration)> {
        // First check all buffers have messages
        if self.buffers.values().any(|b| b.is_empty()) {
            return None;
        }
        self.buffers
            .iter()
            .filter_map(|(key, buffer)| {
                // Get the earliest timestamp
                let ts = buffer.front()?.timestamp();
                Some((key.clone(), ts))
            })
            .max_by_key(|(_, ts)| *ts)
    }

    /// Gets the minimum timestamp among all messages.
    pub fn min_timestamp(&self) -> Option<(K, Duration)> {
        self.buffers
            .iter()
            .filter_map(|(key, buffer)| {
                // Get the earliest timestamp
                let ts = buffer.front()?.timestamp();
                Some((key.clone(), ts))
            })
            .min_by_key(|(_, ts)| *ts)
    }

    /// Checks if every buffer size reaches the limit.
    pub fn is_full(&self) -> bool {
        self.buffers
            .values()
            .all(|buffer| buffer.len() >= self.buf_size)
    }

    /// Checks if every buffer receives at least two messages.
    pub fn is_ready(&self) -> bool {
        self.buffers.values().all(|buffer| buffer.len() >= 2)
    }

    /// Checks if there are buffers which are empty.
    pub fn is_empty(&self) -> bool {
        // self.buffers.values().all(|buffer| buffer.is_empty())
        let buffers = self.buffers.iter();
        for item in buffers {
            let (_key, buffer) = item;
            if buffer.is_empty() {
                return true;
            } else {
                continue;
            }
        }
        false
    }

    /// Checks if all buffers have only one data left.
    pub fn all_one(&self) -> bool {
        self.buffers.values().all(|buffer| buffer.len() == 1)
    }
    /// Remove the message with the minimum timestamp among all
    /// buffers. Returns true if a message is dropped.
    pub fn drop_min(&mut self) -> bool {
        let Some((_, min_ts)) = self.min_timestamp() else {
            return false;
        };

        self.buffers.values_mut().for_each(|buffer| {
            if let Some(front) = buffer.front()
                && front.timestamp() == min_ts
            {
                buffer.pop_front();
            }
        });

        true
    }

    /// Drop expired messages from all buffers based on reference timestamp.
    /// Returns the total number of dropped messages.
    pub fn drop_expired_messages(&mut self, reference_timestamp: Duration) -> usize {
        self.buffers
            .values_mut()
            .map(|buffer| buffer.drop_expired(reference_timestamp))
            .sum()
    }

    /// Insert a message to the queue identified by the key.
    /// Returns Ok(()) on success, or a PushError on failure.
    pub fn push(&mut self, key: K, item: T) -> Result<(), PushError<T>> {
        let timestamp = item.timestamp();

        // Reject messages before commit timestamp
        if let Some(commit_ts) = self.commit_ts
            && commit_ts >= timestamp
        {
            return Err(PushError::LateMessage(item));
        }

        let Some(buffer) = self.buffers.get_mut(&key) else {
            return Err(PushError::UnknownKey(item));
        };

        // Handle buffer capacity based on drop policy
        if buffer.len() >= self.buf_size {
            match self.drop_policy {
                DropPolicy::RejectNew => {
                    return Err(PushError::BufferFull(item));
                }
                DropPolicy::DropOldest => {
                    buffer.pop_front();
                }
            }
        }

        // Add to staleness detector if configured
        if let Some(ref mut staleness_detector) = self.staleness_detector {
            // Use message timeout if available, otherwise use window_size as default staleness timeout
            let staleness_timeout = item
                .timeout()
                .unwrap_or_else(|| self.window_size.unwrap_or(Duration::from_secs(60)));
            staleness_detector.add_message(key.clone(), item.clone(), staleness_timeout);
        }

        buffer.try_push(item).map_err(PushError::OutOfOrder)
    }

    /// Insert a message to the queue, waiting for space if buffer is full.
    ///
    /// This is the blocking version of `push()`. When the buffer is full and
    /// the drop policy is `RejectNew`, this method will wait for space to
    /// become available (via `try_match()` consuming messages).
    ///
    /// # Arguments
    /// * `key` - The key identifying which buffer to push to
    /// * `item` - The message to push
    /// * `timeout` - Maximum time to wait for space. None means wait indefinitely.
    ///
    /// # Returns
    /// * `Ok(())` if the message was successfully pushed
    /// * `Err(PushError::Timeout(item))` if timeout elapsed while waiting
    /// * `Err(PushError::*)` for other push errors (LateMessage, UnknownKey, OutOfOrder)
    ///
    /// # Note
    /// This method requires external code to call `try_match()` to free up buffer space.
    /// If `try_match()` is never called, this will wait until timeout.
    pub async fn push_blocking(
        &mut self,
        key: K,
        item: T,
        timeout: Option<Duration>,
    ) -> Result<(), PushError<T>> {
        // First try non-blocking push
        let mut current_item = item;
        loop {
            match self.push(key.clone(), current_item) {
                Ok(()) => return Ok(()),
                Err(PushError::BufferFull(item)) => {
                    // Wait for space to become available
                    let notified = self.space_notify.notified();
                    current_item = item;

                    if let Some(timeout_duration) = timeout {
                        match tokio::time::timeout(timeout_duration, notified).await {
                            Ok(()) => {
                                // Space might be available, retry
                                continue;
                            }
                            Err(_) => {
                                // Timeout elapsed
                                return Err(PushError::Timeout(current_item));
                            }
                        }
                    } else {
                        // Wait indefinitely
                        notified.await;
                        continue;
                    }
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Process expired messages from staleness detector and remove them from buffers
    pub fn process_staleness_expiration(&mut self) -> usize {
        if let Some(ref mut staleness_detector) = self.staleness_detector {
            let expired_messages = staleness_detector.drain_expired();
            let mut removed_count = 0;

            for (key, expired_message) in expired_messages {
                if let Some(buffer) = self.buffers.get_mut(&key) {
                    // Since we can't remove specific messages from the middle of the buffer,
                    // we'll remove from the front if it matches the expired message
                    // This is a limitation of the current buffer implementation
                    if let Some(front_msg) = buffer.front()
                        && front_msg.timestamp() == expired_message.timestamp()
                    {
                        buffer.pop_front();
                        removed_count += 1;
                    }
                }
            }

            removed_count
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::Buffer;
    use indexmap::IndexMap;
    use std::time::Duration;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestMessage {
        timestamp: Duration,
        data: String,
    }

    impl TestMessage {
        fn new(timestamp_ms: u64, data: &str) -> Self {
            Self {
                timestamp: Duration::from_millis(timestamp_ms),
                data: data.to_string(),
            }
        }
    }

    impl WithTimestamp for TestMessage {
        fn timestamp(&self) -> Duration {
            self.timestamp
        }
    }

    fn create_message(timestamp_ms: u64) -> TestMessage {
        TestMessage::new(timestamp_ms, &format!("msg_{}", timestamp_ms))
    }

    fn create_test_state(buf_size: usize, window_size_ms: u64) -> State<&'static str, TestMessage> {
        let mut buffers = IndexMap::new();
        buffers.insert("A", Buffer::with_capacity(buf_size));
        buffers.insert("B", Buffer::with_capacity(buf_size));

        State {
            buffers,
            commit_ts: Some(Duration::from_millis(1000)),
            buf_size,
            window_size: Some(Duration::from_millis(window_size_ms)),
            drop_policy: DropPolicy::RejectNew,
            feedback_tx: None,
            staleness_detector: None,
            space_notify: Arc::new(Notify::new()),
        }
    }

    fn create_test_state_with_policy(
        buf_size: usize,
        window_size_ms: u64,
        drop_policy: DropPolicy,
    ) -> State<&'static str, TestMessage> {
        let mut buffers = IndexMap::new();
        buffers.insert("A", Buffer::with_capacity(buf_size));
        buffers.insert("B", Buffer::with_capacity(buf_size));

        State {
            buffers,
            commit_ts: Some(Duration::from_millis(1000)),
            buf_size,
            window_size: Some(Duration::from_millis(window_size_ms)),
            drop_policy,
            feedback_tx: None,
            staleness_detector: None,
            space_notify: Arc::new(Notify::new()),
        }
    }

    fn create_test_state_infinite_window(buf_size: usize) -> State<&'static str, TestMessage> {
        let mut buffers = IndexMap::new();
        buffers.insert("A", Buffer::with_capacity(buf_size));
        buffers.insert("B", Buffer::with_capacity(buf_size));

        State {
            buffers,
            commit_ts: Some(Duration::from_millis(1000)),
            buf_size,
            window_size: None,
            drop_policy: DropPolicy::RejectNew,
            feedback_tx: None,
            staleness_detector: None,
            space_notify: Arc::new(Notify::new()),
        }
    }

    #[test]
    fn test_state_is_ready_all_empty() {
        let state = create_test_state(4, 100);
        assert!(!state.is_ready());
    }

    #[test]
    fn test_state_is_ready_some_buffers_insufficient() {
        let mut state = create_test_state(4, 100);

        state.push("A", create_message(1500)).unwrap();
        assert!(!state.is_ready());

        state.push("B", create_message(1500)).unwrap();
        assert!(!state.is_ready());
    }

    #[test]
    fn test_state_is_ready_all_buffers_sufficient() {
        let mut state = create_test_state(4, 100);

        state.push("A", create_message(1500)).unwrap();
        state.push("A", create_message(1600)).unwrap();
        state.push("B", create_message(1510)).unwrap();
        state.push("B", create_message(1610)).unwrap();

        assert!(state.is_ready());
    }

    #[test]
    fn test_state_is_full_no_buffers_full() {
        let mut state = create_test_state(3, 100);

        state.push("A", create_message(1500)).unwrap();
        state.push("B", create_message(1500)).unwrap();

        assert!(!state.is_full());
    }

    #[test]
    fn test_state_is_full_all_buffers_full() {
        let mut state = create_test_state(2, 100);

        state.push("A", create_message(1500)).unwrap();
        state.push("A", create_message(1600)).unwrap();
        state.push("B", create_message(1500)).unwrap();
        state.push("B", create_message(1600)).unwrap();

        assert!(state.is_full());
    }

    #[test]
    fn test_state_is_empty_all_empty() {
        let state = create_test_state(4, 100);
        assert!(state.is_empty());
    }

    #[test]
    fn test_state_is_empty_some_have_data() {
        let mut state = create_test_state(4, 100);

        state.push("A", create_message(1500)).unwrap();
        assert!(state.is_empty()); // Returns true because buffer B is empty
    }

    #[test]
    fn test_state_is_empty_none_empty() {
        let mut state = create_test_state(4, 100);

        state.push("A", create_message(1500)).unwrap();
        state.push("B", create_message(1500)).unwrap();

        assert!(!state.is_empty());
    }

    #[test]
    fn test_state_all_one_false_when_empty() {
        let state = create_test_state(4, 100);
        assert!(!state.all_one());
    }

    #[test]
    fn test_state_all_one_true_when_all_have_one() {
        let mut state = create_test_state(4, 100);

        state.push("A", create_message(1500)).unwrap();
        state.push("B", create_message(1500)).unwrap();

        assert!(state.all_one());
    }

    #[test]
    fn test_state_inf_timestamp_empty_buffers() {
        let state = create_test_state(4, 100);
        assert!(state.inf_timestamp().is_none());
    }

    #[test]
    fn test_state_inf_timestamp_single_message_per_buffer() {
        let mut state = create_test_state(4, 100);

        state.push("A", create_message(1200)).unwrap();
        state.push("B", create_message(1500)).unwrap();

        let (key, ts) = state.inf_timestamp().unwrap();
        assert_eq!(key, "B"); // Maximum of minimums
        assert_eq!(ts, Duration::from_millis(1500));
    }

    #[test]
    fn test_state_sup_timestamp_empty_buffers() {
        let state = create_test_state(4, 100);
        assert!(state.sup_timestamp().is_none());
    }

    #[test]
    fn test_state_min_timestamp_empty_buffers() {
        let state = create_test_state(4, 100);
        assert!(state.min_timestamp().is_none());
    }

    #[test]
    fn test_state_push_valid_insertion() {
        let mut state = create_test_state(4, 100);

        let result = state.push("A", create_message(1500));
        assert!(result.is_ok());
    }

    #[test]
    fn test_state_push_late_message_rejection() {
        let mut state = create_test_state(4, 100);

        let result = state.push("A", create_message(500));
        assert!(result.is_err());
    }

    #[test]
    fn test_state_push_buffer_not_found() {
        let mut state = create_test_state(4, 100);

        let result = state.push("C", create_message(1500));
        assert!(result.is_err());
    }

    #[test]
    fn test_state_drop_min_no_messages() {
        let mut state = create_test_state(4, 100);

        let dropped = state.drop_min();
        assert!(!dropped);
    }

    #[test]
    fn test_state_drop_min_multiple_buffers() {
        let mut state = create_test_state(4, 100);

        state.push("A", create_message(1500)).unwrap();
        state.push("A", create_message(2000)).unwrap();
        state.push("B", create_message(1100)).unwrap();
        state.push("B", create_message(2500)).unwrap();

        let dropped = state.drop_min();
        assert!(dropped);

        let (_, min_ts) = state.min_timestamp().unwrap();
        assert_eq!(min_ts, Duration::from_millis(1500));
    }

    #[test]
    fn test_state_try_match_no_match_possible() {
        let mut state = create_test_state(4, 100);

        // Messages too far apart
        state.push("A", create_message(2000)).unwrap();
        state.push("A", create_message(2600)).unwrap();
        state.push("B", create_message(2500)).unwrap();
        state.push("B", create_message(2700)).unwrap();

        let result = state.try_match();
        assert!(result.is_none());
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestMessageWithTimeout {
        timestamp: Duration,
        data: String,
        timeout: Option<Duration>,
    }

    impl TestMessageWithTimeout {
        fn new(timestamp_ms: u64, data: &str, timeout_ms: Option<u64>) -> Self {
            Self {
                timestamp: Duration::from_millis(timestamp_ms),
                data: data.to_string(),
                timeout: timeout_ms.map(Duration::from_millis),
            }
        }
    }

    impl WithTimestamp for TestMessageWithTimeout {
        fn timestamp(&self) -> Duration {
            self.timestamp
        }

        fn timeout(&self) -> Option<Duration> {
            self.timeout
        }
    }

    fn create_test_state_with_timeout(
        buf_size: usize,
        window_size_ms: u64,
    ) -> State<&'static str, TestMessageWithTimeout> {
        let mut buffers = IndexMap::new();
        buffers.insert("A", Buffer::with_capacity(buf_size));
        buffers.insert("B", Buffer::with_capacity(buf_size));

        State {
            buffers,
            commit_ts: Some(Duration::from_millis(1000)),
            buf_size,
            window_size: Some(Duration::from_millis(window_size_ms)),
            drop_policy: DropPolicy::RejectNew,
            feedback_tx: None,
            staleness_detector: None,
            space_notify: Arc::new(Notify::new()),
        }
    }

    #[test]
    fn test_state_drop_expired_messages_no_timeout() {
        let mut state = create_test_state_with_timeout(4, 100);

        // Add messages without timeout
        let msg1 = TestMessageWithTimeout::new(1500, "msg1", None);
        let msg2 = TestMessageWithTimeout::new(1600, "msg2", None);
        state.push("A", msg1).unwrap();
        state.push("B", msg2).unwrap();

        let dropped = state.drop_expired_messages(Duration::from_millis(3000));
        assert_eq!(dropped, 0);
        assert_eq!(state.buffers["A"].len(), 1);
        assert_eq!(state.buffers["B"].len(), 1);
    }

    #[test]
    fn test_state_drop_expired_messages_with_timeout() {
        let mut state = create_test_state_with_timeout(4, 100);

        // Add messages with different timeouts
        let msg1 = TestMessageWithTimeout::new(1500, "msg1", Some(500)); // expires at 2000ms
        let msg2 = TestMessageWithTimeout::new(1600, "msg2", Some(1000)); // expires at 2600ms
        let msg3 = TestMessageWithTimeout::new(1700, "msg3", Some(2000)); // expires at 3700ms

        state.push("A", msg1).unwrap();
        state.push("A", msg3).unwrap();
        state.push("B", msg2).unwrap();

        // Reference time 2500ms should expire only msg1 (expires at 2000ms)
        // msg2 expires at 2600ms, so it should still be alive
        let dropped = state.drop_expired_messages(Duration::from_millis(2500));
        assert_eq!(dropped, 1); // Only msg1 expired
        assert_eq!(state.buffers["A"].len(), 1); // msg3 remains
        assert_eq!(state.buffers["B"].len(), 1); // msg2 still alive

        // Verify remaining message
        assert_eq!(
            state.buffers["A"].front().unwrap().timestamp(),
            Duration::from_millis(1700)
        );
    }

    #[test]
    fn test_state_drop_expired_messages_mixed() {
        let mut state = create_test_state_with_timeout(4, 100);

        // Mix of timeout and no-timeout messages
        let msg1 = TestMessageWithTimeout::new(1500, "msg1", Some(300)); // expires at 1800ms
        let msg2 = TestMessageWithTimeout::new(1600, "msg2", None); // no timeout
        let msg3 = TestMessageWithTimeout::new(1700, "msg3", Some(1000)); // expires at 2700ms
        state.push("A", msg1).unwrap();
        state.push("A", msg2).unwrap();
        state.push("B", msg3).unwrap();

        // Reference time 2000ms should expire only msg1
        let dropped = state.drop_expired_messages(Duration::from_millis(2000));
        assert_eq!(dropped, 1);
        assert_eq!(state.buffers["A"].len(), 1); // msg2 remains (no timeout)
        assert_eq!(state.buffers["B"].len(), 1); // msg3 remains (not yet expired)
    }

    // ============================================
    // Drop Policy Tests
    // ============================================

    #[test]
    fn test_drop_policy_reject_new_rejects_when_full() {
        let mut state = create_test_state_with_policy(2, 100, DropPolicy::RejectNew);

        // Fill buffer to capacity
        assert!(state.push("A", create_message(1500)).is_ok());
        assert!(state.push("A", create_message(1600)).is_ok());
        assert_eq!(state.buffers["A"].len(), 2);

        // Third message should be rejected
        let result = state.push("A", create_message(1700));
        assert!(matches!(result, Err(PushError::BufferFull(_))));

        // Buffer unchanged
        assert_eq!(state.buffers["A"].len(), 2);
        assert_eq!(
            state.buffers["A"].back().unwrap().timestamp(),
            Duration::from_millis(1600)
        );
    }

    #[test]
    fn test_drop_policy_drop_oldest_evicts_when_full() {
        let mut state = create_test_state_with_policy(2, 100, DropPolicy::DropOldest);

        // Fill buffer to capacity
        assert!(state.push("A", create_message(1500)).is_ok());
        assert!(state.push("A", create_message(1600)).is_ok());
        assert_eq!(state.buffers["A"].len(), 2);

        // Third message should succeed, oldest evicted
        assert!(state.push("A", create_message(1700)).is_ok());

        // Buffer still at capacity, but oldest replaced
        assert_eq!(state.buffers["A"].len(), 2);
        assert_eq!(
            state.buffers["A"].front().unwrap().timestamp(),
            Duration::from_millis(1600)
        );
        assert_eq!(
            state.buffers["A"].back().unwrap().timestamp(),
            Duration::from_millis(1700)
        );
    }

    #[test]
    fn test_drop_policy_drop_oldest_multiple_evictions() {
        let mut state = create_test_state_with_policy(2, 100, DropPolicy::DropOldest);

        // Push 5 messages into buffer of size 2
        for ts in [1500, 1600, 1700, 1800, 1900] {
            assert!(state.push("A", create_message(ts)).is_ok());
        }

        // Should only have the last 2 messages
        assert_eq!(state.buffers["A"].len(), 2);
        assert_eq!(
            state.buffers["A"].front().unwrap().timestamp(),
            Duration::from_millis(1800)
        );
        assert_eq!(
            state.buffers["A"].back().unwrap().timestamp(),
            Duration::from_millis(1900)
        );
    }

    #[test]
    fn test_drop_policy_reject_new_allows_push_after_pop() {
        let mut state = create_test_state_with_policy(2, 100, DropPolicy::RejectNew);

        // Fill buffer
        state.push("A", create_message(1500)).unwrap();
        state.push("A", create_message(1600)).unwrap();

        // Reject when full
        assert!(state.push("A", create_message(1700)).is_err());

        // Pop one message
        state.buffers.get_mut("A").unwrap().pop_front();

        // Now push should succeed
        assert!(state.push("A", create_message(1700)).is_ok());
    }

    // ============================================
    // Infinite Window Tests
    // ============================================

    #[test]
    fn test_infinite_window_no_time_based_dropping() {
        let mut state = create_test_state_infinite_window(10);

        // Push messages with timestamps very far apart (would be dropped with finite window)
        state.push("A", create_message(1500)).unwrap();
        state.push("A", create_message(100_000)).unwrap(); // 100 seconds later
        state.push("B", create_message(50_000)).unwrap();
        state.push("B", create_message(150_000)).unwrap();

        // All messages should still be in buffers
        assert_eq!(state.buffers["A"].len(), 2);
        assert_eq!(state.buffers["B"].len(), 2);

        // try_match should work (no time-based rejection)
        let result = state.try_match();
        assert!(result.is_some());

        let group = result.unwrap();
        assert_eq!(group["A"].timestamp(), Duration::from_millis(1500));
        assert_eq!(group["B"].timestamp(), Duration::from_millis(50_000));
    }

    #[test]
    fn test_infinite_window_matches_any_timestamp_difference() {
        let mut state = create_test_state_infinite_window(10);

        // Messages with 1 hour timestamp difference
        state.push("A", create_message(1500)).unwrap();
        state.push("A", create_message(3_600_000)).unwrap(); // 1 hour
        state.push("B", create_message(1_800_000)).unwrap(); // 30 min
        state.push("B", create_message(5_400_000)).unwrap(); // 1.5 hours

        let result = state.try_match();
        assert!(result.is_some());
    }

    #[test]
    fn test_finite_window_drops_old_messages() {
        let mut state = create_test_state(10, 100); // 100ms window

        // Push message at t=1500
        state.push("A", create_message(1500)).unwrap();
        state.push("A", create_message(1550)).unwrap();

        // Push message at t=2000 for B (500ms after A's first message, outside window)
        state.push("B", create_message(2000)).unwrap();
        state.push("B", create_message(2100)).unwrap();

        // try_match should drop A's messages (outside 100ms window of B)
        let result = state.try_match();

        // No match possible - A's messages are too old for B's window
        assert!(result.is_none());
    }

    #[test]
    fn test_push_error_types() {
        let mut state = create_test_state_with_policy(2, 100, DropPolicy::RejectNew);

        // Test LateMessage error (before commit_ts)
        let result = state.push("A", create_message(500)); // Before commit_ts of 1000
        assert!(matches!(result, Err(PushError::LateMessage(_))));

        // Test UnknownKey error
        let result = state.push("C", create_message(1500)); // Key "C" not registered
        assert!(matches!(result, Err(PushError::UnknownKey(_))));

        // Test BufferFull error
        state.push("A", create_message(1500)).unwrap();
        state.push("A", create_message(1600)).unwrap();
        let result = state.push("A", create_message(1700));
        assert!(matches!(result, Err(PushError::BufferFull(_))));

        // Test OutOfOrder error
        state.push("B", create_message(1500)).unwrap();
        let result = state.push("B", create_message(1400)); // Out of order
        assert!(matches!(result, Err(PushError::OutOfOrder(_))));
    }

    #[test]
    fn test_push_error_into_inner() {
        let msg = create_message(1500);
        let error = PushError::BufferFull(msg.clone());
        let recovered = error.into_inner();
        assert_eq!(recovered.timestamp(), msg.timestamp());
    }

    // ============================================
    // push_blocking() Tests
    // ============================================

    #[tokio::test]
    async fn test_push_blocking_immediate_success_when_space_available() {
        let mut state = create_test_state_with_policy(2, 100, DropPolicy::RejectNew);

        // Buffer has space, should succeed immediately
        let result = state
            .push_blocking("A", create_message(1500), Some(Duration::from_millis(100)))
            .await;
        assert!(result.is_ok());
        assert_eq!(state.buffers["A"].len(), 1);
    }

    #[tokio::test]
    async fn test_push_blocking_timeout_when_buffer_full() {
        let mut state = create_test_state_with_policy(2, 100, DropPolicy::RejectNew);

        // Fill buffer to capacity
        state.push("A", create_message(1500)).unwrap();
        state.push("A", create_message(1600)).unwrap();
        assert_eq!(state.buffers["A"].len(), 2);

        // push_blocking should timeout since no one will free space
        let start = std::time::Instant::now();
        let result = state
            .push_blocking("A", create_message(1700), Some(Duration::from_millis(50)))
            .await;

        // Should have waited approximately 50ms before timing out
        let elapsed = start.elapsed();
        assert!(
            elapsed >= Duration::from_millis(45),
            "Should wait before timeout"
        );
        assert!(
            elapsed < Duration::from_millis(200),
            "Should not wait too long"
        );

        // Result should be Timeout error
        assert!(matches!(result, Err(PushError::Timeout(_))));

        // Buffer should be unchanged
        assert_eq!(state.buffers["A"].len(), 2);
    }

    #[tokio::test]
    async fn test_push_blocking_succeeds_after_space_freed() {
        let mut state = create_test_state_with_policy(2, 100, DropPolicy::RejectNew);

        // Fill both buffers for A and B
        state.push("A", create_message(1500)).unwrap();
        state.push("A", create_message(1550)).unwrap();
        state.push("B", create_message(1520)).unwrap();
        state.push("B", create_message(1570)).unwrap();

        // Clone the notify handle for the background task
        let notify = state.space_notify.clone();

        // Spawn a task that will notify after a short delay (simulating try_match)
        let notify_task = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            notify.notify_waiters();
        });

        // Try push_blocking - it should wait and then succeed after notification
        // Note: We manually pop to make space since we can't call try_match in parallel
        let push_task = async {
            // Wait a bit for the notification
            tokio::time::sleep(Duration::from_millis(25)).await;
            // Manually free space (simulating what try_match does)
            state.buffers.get_mut("A").unwrap().pop_front();
            // Now push should work
            state
                .push_blocking("A", create_message(1700), Some(Duration::from_millis(100)))
                .await
        };

        notify_task.await.unwrap();
        let result = push_task.await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_push_blocking_returns_other_errors_immediately() {
        let mut state = create_test_state_with_policy(2, 100, DropPolicy::RejectNew);

        // Test LateMessage error (before commit_ts) - should return immediately
        let start = std::time::Instant::now();
        let result = state
            .push_blocking("A", create_message(500), Some(Duration::from_millis(100)))
            .await;
        let elapsed = start.elapsed();

        assert!(matches!(result, Err(PushError::LateMessage(_))));
        assert!(
            elapsed < Duration::from_millis(10),
            "Should return immediately for LateMessage"
        );

        // Test UnknownKey error - should return immediately
        let start = std::time::Instant::now();
        let result = state
            .push_blocking("C", create_message(1500), Some(Duration::from_millis(100)))
            .await;
        let elapsed = start.elapsed();

        assert!(matches!(result, Err(PushError::UnknownKey(_))));
        assert!(
            elapsed < Duration::from_millis(10),
            "Should return immediately for UnknownKey"
        );
    }

    #[tokio::test]
    async fn test_push_blocking_with_drop_oldest_never_blocks() {
        let mut state = create_test_state_with_policy(2, 100, DropPolicy::DropOldest);

        // Fill buffer
        state.push("A", create_message(1500)).unwrap();
        state.push("A", create_message(1600)).unwrap();

        // With DropOldest, push_blocking should succeed immediately (oldest evicted)
        let start = std::time::Instant::now();
        let result = state
            .push_blocking("A", create_message(1700), Some(Duration::from_millis(100)))
            .await;
        let elapsed = start.elapsed();

        assert!(result.is_ok());
        assert!(
            elapsed < Duration::from_millis(10),
            "Should not block with DropOldest"
        );

        // Buffer should have newest messages
        assert_eq!(state.buffers["A"].len(), 2);
        assert_eq!(
            state.buffers["A"].front().unwrap().timestamp(),
            Duration::from_millis(1600)
        );
        assert_eq!(
            state.buffers["A"].back().unwrap().timestamp(),
            Duration::from_millis(1700)
        );
    }

    #[tokio::test]
    async fn test_push_blocking_notify_on_try_match() {
        use std::sync::atomic::{AtomicBool, Ordering};

        // Use infinite window to ensure try_match succeeds
        let mut state = create_test_state_infinite_window(2);

        // Fill both buffers
        state.push("A", create_message(1500)).unwrap();
        state.push("A", create_message(1550)).unwrap();
        state.push("B", create_message(1520)).unwrap();
        state.push("B", create_message(1570)).unwrap();

        // Verify try_match triggers notification
        let notify = state.space_notify.clone();
        let notified = Arc::new(AtomicBool::new(false));
        let notified_clone = notified.clone();

        // Set up a listener before try_match
        let listener = tokio::spawn(async move {
            let fut = notify.notified();
            tokio::select! {
                _ = fut => {
                    notified_clone.store(true, Ordering::SeqCst);
                }
                _ = tokio::time::sleep(Duration::from_millis(100)) => {}
            }
        });

        // Give listener time to register
        tokio::time::sleep(Duration::from_millis(5)).await;

        // Call try_match - this should notify waiters
        let result = state.try_match();
        assert!(result.is_some());

        // Wait for listener
        listener.await.unwrap();

        // Notification should have been received
        assert!(
            notified.load(Ordering::SeqCst),
            "try_match should notify waiters"
        );
    }
}
