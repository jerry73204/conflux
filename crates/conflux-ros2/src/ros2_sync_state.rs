//! ROS2-specific synchronization state using move semantics.
//!
//! This module provides [`Ros2SyncState`], a synchronization state machine
//! designed specifically for [`DynamicMessage`] which does not implement `Clone`.
//!
//! The implementation uses the same time-window matching algorithm as conflux-core,
//! but with move semantics instead of cloning. Messages are stored in per-topic
//! buffers and extracted (moved out) when a synchronized group is formed.

use std::collections::VecDeque;
use std::time::Duration;

use indexmap::IndexMap;
use tracing::debug;

use crate::ros2_message::Ros2Message;

/// Per-topic message buffer using move semantics.
///
/// Messages are stored in a FIFO queue. When extracting messages for
/// synchronization, ownership is transferred out of the buffer.
pub struct Ros2Buffer {
    /// Message queue. Uses Option to allow taking ownership without removing.
    messages: VecDeque<Ros2Message>,

    /// Maximum number of messages to buffer.
    capacity: usize,
}

impl Ros2Buffer {
    /// Create a new buffer with the given capacity.
    pub fn new(capacity: usize) -> Self {
        Self {
            messages: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    /// Push a message to the buffer.
    ///
    /// If the buffer is at capacity, the oldest message is dropped and returned.
    pub fn push(&mut self, msg: Ros2Message) -> Option<Ros2Message> {
        let dropped = if self.messages.len() >= self.capacity {
            self.messages.pop_front()
        } else {
            None
        };
        self.messages.push_back(msg);
        dropped
    }

    /// Peek at the front message's timestamp without taking ownership.
    pub fn front_timestamp(&self) -> Option<Duration> {
        self.messages.front().map(|m| m.timestamp)
    }

    /// Peek at the back message's timestamp without taking ownership.
    pub fn back_timestamp(&self) -> Option<Duration> {
        self.messages.back().map(|m| m.timestamp)
    }

    /// Take the front message, transferring ownership to the caller.
    pub fn take_front(&mut self) -> Option<Ros2Message> {
        self.messages.pop_front()
    }

    /// Number of messages currently buffered.
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// Check if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// Drop all messages with timestamp strictly before the threshold.
    ///
    /// Returns the number of messages dropped.
    pub fn drop_before(&mut self, threshold: Duration) -> usize {
        let mut dropped = 0;
        while let Some(front) = self.messages.front() {
            if front.timestamp < threshold {
                self.messages.pop_front();
                dropped += 1;
            } else {
                break;
            }
        }
        dropped
    }
}

/// Synchronization state for ROS2 DynamicMessages.
///
/// Implements time-window based synchronization using move semantics.
/// Messages from multiple topics are buffered and matched when they
/// fall within the configured time window.
///
/// # Algorithm
///
/// The matching algorithm works as follows:
/// 1. Find `inf_ts` = maximum of front timestamps across all buffers
/// 2. Check if all front messages are within `window_size` of `inf_ts`
/// 3. If yes, extract one message from each buffer as a synchronized group
/// 4. If no, drop the oldest message (smallest timestamp) and retry
///
/// This ensures that synchronized groups contain messages that are
/// temporally close to each other.
pub struct Ros2SyncState {
    /// Per-topic message buffers, keyed by topic name.
    buffers: IndexMap<String, Ros2Buffer>,

    /// Time window for grouping messages.
    window_size: Duration,

    /// Buffer capacity per topic (stored for stats/debugging).
    #[allow(dead_code)]
    buffer_size: usize,

    /// Commit timestamp - messages with timestamp < commit_ts are rejected.
    /// This advances as synchronized groups are emitted.
    commit_ts: Duration,

    /// Statistics: number of synchronized groups emitted.
    groups_emitted: u64,

    /// Statistics: number of late messages rejected.
    late_rejected: u64,

    /// Statistics: number of messages dropped due to buffer overflow or mismatch.
    dropped: u64,
}

impl Ros2SyncState {
    /// Create a new synchronization state.
    ///
    /// # Arguments
    ///
    /// * `topics` - List of topic names to synchronize
    /// * `window_size` - Maximum time difference between synchronized messages
    /// * `buffer_size` - Maximum messages to buffer per topic
    pub fn new(topics: Vec<String>, window_size: Duration, buffer_size: usize) -> Self {
        let buffers = topics
            .into_iter()
            .map(|t| (t, Ros2Buffer::new(buffer_size)))
            .collect();

        Self {
            buffers,
            window_size,
            buffer_size,
            commit_ts: Duration::ZERO,
            groups_emitted: 0,
            late_rejected: 0,
            dropped: 0,
        }
    }

    /// Push a message to the appropriate topic buffer.
    ///
    /// Returns `Err(msg)` if:
    /// - The topic is not registered
    /// - The message timestamp is before the commit timestamp (late message)
    pub fn push(&mut self, msg: Ros2Message) -> Result<(), Ros2Message> {
        // Reject late messages
        if msg.timestamp < self.commit_ts {
            self.late_rejected += 1;
            debug!(
                topic = %msg.topic,
                msg_ts = ?msg.timestamp,
                commit_ts = ?self.commit_ts,
                "Rejected late message"
            );
            return Err(msg);
        }

        // Find the buffer for this topic
        let Some(buffer) = self.buffers.get_mut(&msg.topic) else {
            debug!(topic = %msg.topic, "Unknown topic, rejecting message");
            return Err(msg);
        };

        // Push to buffer, tracking if we dropped an old message
        if let Some(_dropped) = buffer.push(msg) {
            self.dropped += 1;
            debug!("Dropped oldest message due to buffer overflow");
        }

        Ok(())
    }

    /// Check if all buffers have at least one message.
    pub fn is_ready(&self) -> bool {
        self.buffers.values().all(|b| !b.is_empty())
    }

    /// Check if all buffers have at least two messages.
    ///
    /// Having two messages per buffer allows the algorithm to make progress
    /// even when the front messages don't match.
    pub fn is_well_filled(&self) -> bool {
        self.buffers.values().all(|b| b.len() >= 2)
    }

    /// Attempt to extract a synchronized group of messages.
    ///
    /// Returns `Some(group)` if all front messages are within the time window.
    /// Returns `None` if matching is not possible (will drop oldest and retry).
    pub fn try_match(&mut self) -> Option<IndexMap<String, Ros2Message>> {
        if !self.is_ready() {
            return None;
        }

        // Find inf_ts = max of front timestamps
        let inf_ts = self
            .buffers
            .values()
            .filter_map(|b| b.front_timestamp())
            .max()?;

        // Check if all fronts are within window of inf_ts
        let all_within_window = self.buffers.values().all(|b| {
            b.front_timestamp()
                .map(|ts| {
                    // ts should be <= inf_ts, and inf_ts - ts <= window_size
                    inf_ts.saturating_sub(ts) <= self.window_size
                })
                .unwrap_or(false)
        });

        if !all_within_window {
            // Find and drop the oldest message (smallest front timestamp)
            let min_topic = self
                .buffers
                .iter()
                .filter_map(|(t, b)| b.front_timestamp().map(|ts| (t.clone(), ts)))
                .min_by_key(|(_, ts)| *ts)?
                .0;

            if let Some(buffer) = self.buffers.get_mut(&min_topic) {
                if let Some(_dropped) = buffer.take_front() {
                    self.dropped += 1;
                    debug!(
                        topic = %min_topic,
                        "Dropped oldest message to advance synchronization"
                    );
                }
            }
            return None;
        }

        // All messages are within window - extract synchronized group
        let mut group = IndexMap::new();
        let mut min_ts = Duration::MAX;

        for (topic, buffer) in self.buffers.iter_mut() {
            if let Some(msg) = buffer.take_front() {
                min_ts = min_ts.min(msg.timestamp);
                group.insert(topic.clone(), msg);
            }
        }

        // Update commit timestamp to prevent accepting older messages
        self.commit_ts = min_ts;
        self.groups_emitted += 1;

        debug!(
            group_num = self.groups_emitted,
            min_ts = ?min_ts,
            num_messages = group.len(),
            "Emitting synchronized group"
        );

        Some(group)
    }

    /// Get synchronization statistics.
    pub fn stats(&self) -> SyncStats {
        SyncStats {
            groups_emitted: self.groups_emitted,
            late_rejected: self.late_rejected,
            dropped: self.dropped,
            commit_ts: self.commit_ts,
            buffer_sizes: self
                .buffers
                .iter()
                .map(|(t, b)| (t.clone(), b.len()))
                .collect(),
        }
    }

    /// Get the current commit timestamp.
    pub fn commit_timestamp(&self) -> Duration {
        self.commit_ts
    }

    /// Get the number of registered topics.
    pub fn num_topics(&self) -> usize {
        self.buffers.len()
    }
}

/// Statistics about the synchronization state.
#[derive(Debug, Clone)]
pub struct SyncStats {
    /// Number of synchronized groups emitted.
    pub groups_emitted: u64,

    /// Number of late messages rejected.
    pub late_rejected: u64,

    /// Number of messages dropped (overflow or mismatch).
    pub dropped: u64,

    /// Current commit timestamp.
    pub commit_ts: Duration,

    /// Current buffer sizes per topic.
    pub buffer_sizes: IndexMap<String, usize>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rclrs::DynamicMessage;

    // Note: These tests require a ROS2 environment to create DynamicMessages.
    // For unit testing without ROS2, we would need mock messages.

    #[test]
    fn test_buffer_basic_operations() {
        // Test buffer without DynamicMessage (just the data structure logic)
        let mut buffer: VecDeque<Duration> = VecDeque::with_capacity(3);

        // Simulate push behavior
        buffer.push_back(Duration::from_millis(100));
        buffer.push_back(Duration::from_millis(200));
        assert_eq!(buffer.len(), 2);

        // Simulate take_front
        let front = buffer.pop_front();
        assert_eq!(front, Some(Duration::from_millis(100)));
        assert_eq!(buffer.len(), 1);
    }

    #[test]
    fn test_sync_state_creation() {
        let topics = vec![
            "/topic_a".to_string(),
            "/topic_b".to_string(),
        ];
        let state = Ros2SyncState::new(topics, Duration::from_millis(50), 10);

        assert_eq!(state.num_topics(), 2);
        assert!(!state.is_ready()); // No messages yet
        assert_eq!(state.commit_timestamp(), Duration::ZERO);
    }
}
