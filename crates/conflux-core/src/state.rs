use crate::{
    buffer::Buffer,
    staleness::StalenessDetector,
    types::{Feedback, Key, WithTimestamp},
};
use indexmap::IndexMap;
use std::time::Duration;
use tokio::sync::watch;

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
    /// within.
    pub window_size: Duration,

    /// The sender where feedback messages are sent to.
    pub feedback_tx: Option<watch::Sender<Feedback<K>>>,

    /// Optional staleness detector for real-time message expiration
    pub staleness_detector: Option<StalenessDetector<K, T>>,
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
    pub fn try_match(&mut self) -> Option<IndexMap<K, T>> {
        let inf_ts = loop {
            let (_, inf_ts) = self.inf_timestamp()?;

            // Checking all buffers have only one data left.
            // Make sure (sup - inf >= window_size). If not, it needs to
            // wait for more messages.
            let (_, sup_ts) = self.sup_timestamp()?;
            if !self.all_one() && inf_ts + self.window_size > sup_ts {
                return None;
            }

            let window_start = inf_ts.saturating_sub(self.window_size);

            // Drop messages before the time window.
            let dropped = self.buffers.values_mut().any(|buffer| {
                let count = buffer.drop_before(window_start);
                count > 0
            });

            if !dropped {
                break inf_ts;
            }
        };

        // let window_start = inf_ts.saturating_sub(self.window_size);
        let window_end = inf_ts.saturating_add(self.window_size);

        let items: IndexMap<_, _> = self
            .buffers
            .iter_mut()
            .map(|(key, buffer)| {
                // find the first candidate that is within the window
                let item = buffer.pop_front().unwrap();
                assert!(item.timestamp() <= window_end);
                (key.clone(), item)
            })
            .collect();

        // update commit timestamp
        let new_commit_ts = items.values().map(|item| item.timestamp()).min().unwrap();
        self.commit_ts = Some(new_commit_ts);

        Some(items)
    }

    /// Gets the minimum of the maximum timestamps from each buffer.
    pub fn sup_timestamp(&self) -> Option<(K, Duration)> {
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
    pub fn inf_timestamp(&self) -> Option<(K, Duration)> {
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

    /// Insert a message to the queue identified by the key. It
    /// returns true if the message is successfully inserted.
    pub fn push(&mut self, key: K, item: T) -> Result<(), T> {
        let timestamp = item.timestamp();

        match self.commit_ts {
            Some(commit_ts) if commit_ts >= timestamp => return Err(item),
            _ => {}
        }

        let Some(buffer) = self.buffers.get_mut(&key) else {
            return Err(item);
        };

        // Add to staleness detector if configured
        if let Some(ref mut staleness_detector) = self.staleness_detector {
            // Use message timeout if available, otherwise use window_size as default staleness timeout
            let staleness_timeout = item.timeout().unwrap_or(self.window_size);
            staleness_detector.add_message(key.clone(), item.clone(), staleness_timeout);
        }

        buffer.try_push(item)
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
            window_size: Duration::from_millis(window_size_ms),
            feedback_tx: None,
            staleness_detector: None,
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
            window_size: Duration::from_millis(window_size_ms),
            feedback_tx: None,
            staleness_detector: None,
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
}
