use crate::types::WithTimestamp;
use std::{collections::VecDeque, time::Duration};

/// A buffer to store a sequence of messages with monotonically
/// increasing timestamps.
#[derive(Debug)]
pub struct Buffer<T>
where
    T: WithTimestamp,
{
    buffer: VecDeque<T>,
    last_ts: Option<Duration>,
}

impl<T> Buffer<T>
where
    T: WithTimestamp,
{
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buffer: VecDeque::with_capacity(capacity),
            last_ts: None,
        }
    }

    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    pub fn front(&self) -> Option<&T> {
        self.buffer.front()
    }

    pub fn back(&self) -> Option<&T> {
        self.buffer.back()
    }

    pub fn pop_front(&mut self) -> Option<T> {
        self.buffer.pop_front()
    }

    pub fn front_entry(&mut self) -> Option<FrontEntry<'_, T>> {
        let item = self.buffer.pop_front()?;
        Some(FrontEntry {
            buffer: self,
            item: Some(item),
        })
    }

    // pub fn back_entry(&mut self) -> Option<BackEntry<'_, T>> {
    //     let item = self.buffer.pop_back()?;
    //     Some(BackEntry {
    //         buffer: self,
    //         item: Some(item),
    //     })
    // }

    // pub fn pop_back(&mut self) -> Option<T> {
    //     self.buffer.pop_back()
    // }

    // pub fn front_ts(&self) -> Option<Duration> {
    //     self.buffer
    //         .front()
    //         .map(|item| item.timestamp())
    //         .or(self.last_ts)
    // }

    // pub fn last_ts(&self) -> Option<Duration> {
    //     self.last_ts
    // }

    /// Drops messages before the a specific timestamp and returns the
    /// number of dropped messages.
    pub fn drop_before(&mut self, ts: Duration) -> usize {
        let mut count = 0;

        loop {
            let Some(entry) = self.front_entry() else {
                break;
            };

            if entry.value().timestamp() >= ts {
                break;
            } else {
                let _ = entry.take();
                count += 1;
            }
        }

        count
    }

    /// Drop expired messages based on their timeout and reference timestamp.
    /// Returns the number of dropped messages.
    pub fn drop_expired(&mut self, reference_timestamp: Duration) -> usize {
        let mut count = 0;

        loop {
            let Some(entry) = self.front_entry() else {
                break;
            };

            let message = entry.value();
            let message_time = message.timestamp();

            // Check if message has expired based on its timeout
            if let Some(timeout) = message.timeout()
                && reference_timestamp.saturating_sub(message_time) >= timeout
            {
                let _ = entry.take();
                count += 1;
                continue; // Continue checking next message
            }

            // Message not expired or has no timeout - stop here since messages are ordered
            break;
        }

        count
    }

    /// Try to push a message into the buffer.
    ///
    /// If the timestamp on the message is below that of the
    /// previously inserted message, the message is dropped and the
    /// method returns false. Otherwise, it stores and message and
    /// returns true.
    pub fn try_push(&mut self, item: T) -> Result<(), T> {
        let timestamp = item.timestamp();

        // Ensure that the inserted message has greater timestamp than
        // the latest timestamp.
        match self.last_ts {
            Some(last_ts) if last_ts >= timestamp => return Err(item),
            _ => {}
        }

        self.last_ts = Some(timestamp);
        self.buffer.push_back(item);
        Ok(())
    }
}

pub struct FrontEntry<'a, T>
where
    T: WithTimestamp,
{
    buffer: &'a mut Buffer<T>,
    item: Option<T>,
}

impl<'a, T> FrontEntry<'a, T>
where
    T: WithTimestamp,
{
    pub fn take(mut self) -> T {
        self.item.take().unwrap()
    }

    pub fn value(&self) -> &T {
        self.item.as_ref().unwrap()
    }
}

impl<'a, T> Drop for FrontEntry<'a, T>
where
    T: WithTimestamp,
{
    fn drop(&mut self) {
        if let Some(item) = self.item.take() {
            self.buffer.buffer.push_front(item);
        }
    }
}

// pub struct BackEntry<'a, T>
// where
//     T: WithTimestamp,
// {
//     buffer: &'a mut Buffer<T>,
//     item: Option<T>,
// }

// impl<'a, T> BackEntry<'a, T>
// where
//     T: WithTimestamp,
// {
//     pub fn take(mut self) -> T {
//         self.item.take().unwrap()
//     }

//     pub fn value(&self) -> &T {
//         self.item.as_ref().unwrap()
//     }
// }

// impl<'a, T> Drop for BackEntry<'a, T>
// where
//     T: WithTimestamp,
// {
//     fn drop(&mut self) {
//         if let Some(item) = self.item.take() {
//             self.buffer.buffer.push_back(item);
//         }
//     }
// }

#[cfg(test)]
mod tests {
    use super::*;
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

    fn create_messages(timestamps_ms: &[u64]) -> Vec<TestMessage> {
        timestamps_ms.iter().map(|&ts| create_message(ts)).collect()
    }

    #[test]
    fn test_buffer_with_capacity() {
        let buffer: Buffer<TestMessage> = Buffer::with_capacity(5);
        assert_eq!(buffer.len(), 0);
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_buffer_len_and_is_empty() {
        let mut buffer = Buffer::with_capacity(3);

        assert_eq!(buffer.len(), 0);
        assert!(buffer.is_empty());

        let msg1 = create_message(1000);
        buffer.try_push(msg1).unwrap();

        assert_eq!(buffer.len(), 1);
        assert!(!buffer.is_empty());

        let msg2 = create_message(2000);
        buffer.try_push(msg2).unwrap();

        assert_eq!(buffer.len(), 2);
        assert!(!buffer.is_empty());
    }

    #[test]
    fn test_buffer_front_and_back() {
        let mut buffer = Buffer::with_capacity(3);

        assert!(buffer.front().is_none());
        assert!(buffer.back().is_none());

        let msg1 = create_message(1000);
        let msg2 = create_message(2000);
        let msg3 = create_message(3000);

        buffer.try_push(msg1.clone()).unwrap();
        assert_eq!(
            buffer.front().unwrap().timestamp(),
            Duration::from_millis(1000)
        );
        assert_eq!(
            buffer.back().unwrap().timestamp(),
            Duration::from_millis(1000)
        );

        buffer.try_push(msg2.clone()).unwrap();
        assert_eq!(
            buffer.front().unwrap().timestamp(),
            Duration::from_millis(1000)
        );
        assert_eq!(
            buffer.back().unwrap().timestamp(),
            Duration::from_millis(2000)
        );

        buffer.try_push(msg3.clone()).unwrap();
        assert_eq!(
            buffer.front().unwrap().timestamp(),
            Duration::from_millis(1000)
        );
        assert_eq!(
            buffer.back().unwrap().timestamp(),
            Duration::from_millis(3000)
        );
    }

    #[test]
    fn test_buffer_pop_front() {
        let mut buffer = Buffer::with_capacity(3);

        assert!(buffer.pop_front().is_none());

        let msg1 = create_message(1000);
        let msg2 = create_message(2000);

        buffer.try_push(msg1.clone()).unwrap();
        buffer.try_push(msg2.clone()).unwrap();

        let popped = buffer.pop_front().unwrap();
        assert_eq!(popped.timestamp(), Duration::from_millis(1000));
        assert_eq!(buffer.len(), 1);

        let popped = buffer.pop_front().unwrap();
        assert_eq!(popped.timestamp(), Duration::from_millis(2000));
        assert_eq!(buffer.len(), 0);
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_buffer_drop_before_empty() {
        let mut buffer: Buffer<TestMessage> = Buffer::with_capacity(3);
        let dropped = buffer.drop_before(Duration::from_millis(1000));
        assert_eq!(dropped, 0);
    }

    #[test]
    fn test_buffer_drop_before_single_message() {
        let mut buffer = Buffer::with_capacity(3);
        let msg = create_message(1000);
        buffer.try_push(msg).unwrap();

        let dropped = buffer.drop_before(Duration::from_millis(1500));
        assert_eq!(dropped, 1);
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_buffer_drop_before_multiple_messages() {
        let mut buffer = Buffer::with_capacity(5);
        let messages = create_messages(&[1000, 1500, 2000, 2500, 3000]);

        for msg in messages {
            buffer.try_push(msg).unwrap();
        }

        let dropped = buffer.drop_before(Duration::from_millis(2200));
        assert_eq!(dropped, 3);
        assert_eq!(buffer.len(), 2);
        assert_eq!(
            buffer.front().unwrap().timestamp(),
            Duration::from_millis(2500)
        );
    }

    #[test]
    fn test_buffer_try_push_valid_timestamp() {
        let mut buffer = Buffer::with_capacity(3);

        let msg1 = create_message(1000);
        let result = buffer.try_push(msg1);
        assert!(result.is_ok());
        assert_eq!(buffer.len(), 1);

        let msg2 = create_message(2000);
        let result = buffer.try_push(msg2);
        assert!(result.is_ok());
        assert_eq!(buffer.len(), 2);
    }

    #[test]
    fn test_buffer_try_push_out_of_order_rejection() {
        let mut buffer = Buffer::with_capacity(3);

        let msg1 = create_message(2000);
        buffer.try_push(msg1).unwrap();

        let msg2 = create_message(1000);
        let result = buffer.try_push(msg2);
        assert!(result.is_err());
        assert_eq!(buffer.len(), 1);
    }

    #[test]
    fn test_buffer_allows_unlimited_growth() {
        let mut buffer = Buffer::with_capacity(2);

        let msg1 = create_message(1000);
        let msg2 = create_message(2000);
        let msg3 = create_message(3000);

        assert!(buffer.try_push(msg1).is_ok());
        assert!(buffer.try_push(msg2).is_ok());

        // Buffer doesn't enforce capacity in try_push - allows unlimited growth
        let result = buffer.try_push(msg3);
        assert!(result.is_ok());
        assert_eq!(buffer.len(), 3);
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

    #[test]
    fn test_drop_expired_no_timeout_messages() {
        let mut buffer = Buffer::with_capacity(3);

        // Messages without timeout should not be dropped
        let msg1 = TestMessageWithTimeout::new(1000, "msg1", None);
        let msg2 = TestMessageWithTimeout::new(2000, "msg2", None);
        buffer.try_push(msg1).unwrap();
        buffer.try_push(msg2).unwrap();

        let dropped = buffer.drop_expired(Duration::from_millis(5000));
        assert_eq!(dropped, 0);
        assert_eq!(buffer.len(), 2);
    }

    #[test]
    fn test_drop_expired_with_timeout_not_expired() {
        let mut buffer = Buffer::with_capacity(3);

        // Messages with timeout but not expired
        let msg1 = TestMessageWithTimeout::new(1000, "msg1", Some(1000)); // timeout 1s
        let msg2 = TestMessageWithTimeout::new(2000, "msg2", Some(2000)); // timeout 2s
        buffer.try_push(msg1).unwrap();
        buffer.try_push(msg2).unwrap();

        // Reference time is 2500ms, msg1 (1000ms + 1000ms timeout = 2000ms) is expired
        // but msg2 (2000ms + 2000ms timeout = 4000ms) is not expired
        let dropped = buffer.drop_expired(Duration::from_millis(2500));
        assert_eq!(dropped, 1);
        assert_eq!(buffer.len(), 1);

        // Remaining message should be msg2
        assert_eq!(
            buffer.front().unwrap().timestamp(),
            Duration::from_millis(2000)
        );
    }

    #[test]
    fn test_drop_expired_all_expired() {
        let mut buffer = Buffer::with_capacity(3);

        let msg1 = TestMessageWithTimeout::new(1000, "msg1", Some(500)); // expires at 1500ms
        let msg2 = TestMessageWithTimeout::new(2000, "msg2", Some(1000)); // expires at 3000ms
        buffer.try_push(msg1).unwrap();
        buffer.try_push(msg2).unwrap();

        // Reference time is 4000ms, both messages should be expired
        let dropped = buffer.drop_expired(Duration::from_millis(4000));
        assert_eq!(dropped, 2);
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_drop_expired_mixed_timeout_and_no_timeout() {
        let mut buffer = Buffer::with_capacity(4);

        let msg1 = TestMessageWithTimeout::new(1000, "msg1", Some(500)); // expires at 1500ms
        let msg2 = TestMessageWithTimeout::new(2000, "msg2", None); // no timeout
        let msg3 = TestMessageWithTimeout::new(3000, "msg3", Some(1000)); // expires at 4000ms
        buffer.try_push(msg1).unwrap();
        buffer.try_push(msg2).unwrap();
        buffer.try_push(msg3).unwrap();

        // Reference time is 2000ms, only msg1 should be expired
        let dropped = buffer.drop_expired(Duration::from_millis(2000));
        assert_eq!(dropped, 1);
        assert_eq!(buffer.len(), 2);

        // Should have msg2 and msg3 remaining
        assert_eq!(
            buffer.front().unwrap().timestamp(),
            Duration::from_millis(2000)
        );
    }
}
