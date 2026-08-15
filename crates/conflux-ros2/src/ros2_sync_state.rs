//! ROS2 synchronization state.
//!
//! [`Ros2SyncState`] adapts [`conflux_core::State`] for ROS 2 messages. It owns
//! no matching logic of its own -- it registers topics, forwards pushes, and
//! turns the core's output into an `IndexMap` keyed by topic.
//!
//! # Why this is an adapter and not an algorithm (H-14)
//!
//! It used to be an algorithm: its own buffers, its own `try_match`, its own
//! drop logic, sharing nothing with `conflux_core::State`. That made the
//! standalone `conflux_node` run a third set of semantics, after `sync()` and
//! the C FFI.
//!
//! To be precise about what that cost: the old implementation was **not**
//! vulnerable to C-05. It had no wait-for-spread rule -- it dropped the oldest
//! message whenever the fronts did not all fit the window -- so it could not
//! wedge the way the core did. Replayed against the C-05 scenario it recovers
//! fully (40/40 pushes accepted, 20 groups), exactly as the fixed core now does.
//! The divergence was real but it ran in the other direction: this pipeline
//! discarded data more eagerly than the core, and gave a third answer on the
//! same input.
//!
//! What it genuinely lacked was everything added around the algorithm: no
//! `reset` for a restarted clock (M-22), no way to report why it was not
//! matching (M-23), and no meaningful tests -- the ones it shipped with
//! exercised a bare `VecDeque` rather than any of this code.
//!
//! The duplication existed for a real reason: [`Ros2Message`] wraps a
//! `DynamicMessage`, which is not `Clone`, and `conflux_core::State` used to
//! require `T: Clone`. That bound existed only for the staleness detector, which
//! was removed with M-17..M-21, so the core now accepts move-only messages and
//! the duplicate has nothing left to justify it.

use std::time::Duration;

use conflux_core::{
    DropPolicy, MatchStatus, WithTimestamp, buffer::Buffer, state::PushError, state::State,
};
use indexmap::IndexMap;
use std::sync::Arc;
use tokio::sync::Notify;

use crate::ros2_message::Ros2Message;

/// Synchronization state for ROS 2 messages.
///
/// Generic over the message type so it can be exercised without a live ROS 2
/// environment; production instantiates it with [`Ros2Message`], which is the
/// default.
pub struct Ros2SyncState<M = Ros2Message>
where
    M: WithTimestamp,
{
    /// The shared matching core. All windowing, dropping and forced-progress
    /// rules live here and nowhere else.
    inner: State<String, M>,

    groups_emitted: u64,
    late_rejected: u64,
    dropped: u64,
}

impl<M> Ros2SyncState<M>
where
    M: WithTimestamp,
{
    /// Create a synchronization state for the given topics.
    ///
    /// Uses [`DropPolicy::DropOldest`], preserving the behaviour this type had
    /// before it became an adapter: its buffers always evicted the oldest
    /// message on overflow rather than rejecting the new one.
    pub fn new(topics: Vec<String>, window_size: Duration, buffer_size: usize) -> Self {
        Self::with_drop_policy(topics, window_size, buffer_size, DropPolicy::DropOldest)
    }

    /// Create a synchronization state with an explicit overflow policy.
    pub fn with_drop_policy(
        topics: Vec<String>,
        window_size: Duration,
        buffer_size: usize,
        drop_policy: DropPolicy,
    ) -> Self {
        let buffers: IndexMap<String, Buffer<M>> = topics
            .into_iter()
            .map(|topic| (topic, Buffer::with_capacity(buffer_size)))
            .collect();

        Self {
            inner: State {
                buffers,
                commit_ts: None,
                buf_size: buffer_size,
                window_size: Some(window_size),
                drop_policy,
                feedback_tx: None,
                space_notify: Arc::new(Notify::new()),
            },
            groups_emitted: 0,
            late_rejected: 0,
            dropped: 0,
        }
    }

    /// Buffer a message under the given topic.
    ///
    /// Returns the message back on rejection -- unknown topic, a timestamp at or
    /// before the last emitted group, a non-monotonic timestamp, or a full
    /// buffer under `RejectNew`.
    pub fn push(&mut self, topic: &str, msg: M) -> Result<(), M> {
        match self.inner.push(topic.to_string(), msg) {
            Ok(()) => Ok(()),
            Err(err) => {
                match err {
                    PushError::LateMessage(_) | PushError::OutOfOrder(_) => {
                        self.late_rejected += 1
                    }
                    _ => self.dropped += 1,
                }
                Err(err.into_inner())
            }
        }
    }

    /// Try to extract the next synchronized group.
    ///
    /// Delegates to [`State::advance`], so this inherits the shared rule: match
    /// if a group is available, and force progress when a buffer is full and
    /// nothing can pair (C-05). There is no second copy of that rule to drift.
    pub fn try_match(&mut self) -> Option<IndexMap<String, M>> {
        let group = self.inner.advance()?;
        self.groups_emitted += 1;
        Some(group)
    }

    /// Why the matcher is or is not emitting (M-23).
    pub fn match_status(&self) -> MatchStatus {
        self.inner.match_status()
    }

    /// Discard all buffered messages and forget all timestamp history (M-22).
    ///
    /// Use after the source restarts its clock -- a bag loop, a sim-time reset,
    /// or a reconnecting sensor.
    pub fn reset(&mut self) {
        self.inner.reset();
    }

    /// True when every topic has at least one buffered message.
    pub fn is_ready(&self) -> bool {
        !self.inner.has_empty_buffer()
    }

    /// True when every topic has at least two buffered messages.
    pub fn is_well_filled(&self) -> bool {
        self.inner.is_ready()
    }

    /// Synchronization statistics.
    pub fn stats(&self) -> SyncStats {
        SyncStats {
            groups_emitted: self.groups_emitted,
            late_rejected: self.late_rejected,
            dropped: self.dropped,
            commit_ts: self.inner.commit_ts.unwrap_or(Duration::ZERO),
            buffer_sizes: self
                .inner
                .buffers
                .iter()
                .map(|(topic, buffer)| (topic.clone(), buffer.len()))
                .collect(),
        }
    }

    /// The timestamp of the most recently emitted group.
    pub fn commit_timestamp(&self) -> Duration {
        self.inner.commit_ts.unwrap_or(Duration::ZERO)
    }

    /// Number of registered topics.
    pub fn num_topics(&self) -> usize {
        self.inner.buffers.len()
    }
}

impl Ros2SyncState<Ros2Message> {
    /// Buffer a message, taking its topic from the message itself.
    ///
    /// Convenience for the ROS 2 path, where every message already carries the
    /// topic it arrived on.
    pub fn push_message(&mut self, msg: Ros2Message) -> Result<(), Ros2Message> {
        let topic = msg.topic.clone();
        self.push(&topic, msg)
    }
}

/// Statistics about the synchronization state.
#[derive(Debug, Clone)]
pub struct SyncStats {
    /// Number of synchronized groups emitted.
    pub groups_emitted: u64,

    /// Number of messages rejected as late or out-of-order.
    pub late_rejected: u64,

    /// Number of messages dropped (overflow or unknown topic).
    pub dropped: u64,

    /// Timestamp of the most recently emitted group.
    pub commit_ts: Duration,

    /// Current buffer sizes per topic.
    pub buffer_sizes: IndexMap<String, usize>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use conflux_core::BlockedReason;

    #[derive(Debug, PartialEq, Eq)]
    struct TestMsg(Duration);

    impl TestMsg {
        fn at(ms: u64) -> Self {
            Self(Duration::from_millis(ms))
        }
    }

    impl WithTimestamp for TestMsg {
        fn timestamp(&self) -> Duration {
            self.0
        }
    }

    fn state(window_ms: u64, buffer: usize) -> Ros2SyncState<TestMsg> {
        Ros2SyncState::new(
            vec!["a".to_string(), "b".to_string()],
            Duration::from_millis(window_ms),
            buffer,
        )
    }

    #[test]
    fn emits_a_group_for_aligned_messages() {
        let mut st = state(50, 8);
        st.push("a", TestMsg::at(1000)).unwrap();
        st.push("b", TestMsg::at(1005)).unwrap();

        let group = st.try_match().expect("an aligned pair should form a group");
        assert_eq!(group.len(), 2);
        assert_eq!(st.stats().groups_emitted, 1);
    }

    /// C-05, through this pipeline: after a divergence the buffers fill with
    /// mutually unmatchable messages. The core forces progress, so this must too.
    #[test]
    fn recovers_after_a_stream_divergence() {
        let mut st = state(50, 2);

        for ts in [1000u64, 1010] {
            let _ = st.push("a", TestMsg::at(ts));
        }
        for ts in [5000u64, 5010] {
            let _ = st.push("b", TestMsg::at(ts));
        }
        while st.try_match().is_some() {}

        let mut groups = 0;
        for i in 0..20u64 {
            let t = 6000 + i * 33;
            let _ = st.push("a", TestMsg::at(t));
            let _ = st.push("b", TestMsg::at(t + 5));
            while st.try_match().is_some() {
                groups += 1;
            }
        }

        assert!(
            groups > 0,
            "wedged: aligned data after a divergence produced no groups"
        );
    }

    /// M-22, through this pipeline.
    #[test]
    fn reset_revives_a_stream_after_a_clock_jump() {
        let mut st = state(50, 8);
        st.push("a", TestMsg::at(5000)).unwrap();
        st.push("b", TestMsg::at(5010)).unwrap();
        assert!(st.try_match().is_some());

        assert!(
            st.push("a", TestMsg::at(1000)).is_err(),
            "a backwards timestamp is rejected before the reset"
        );

        st.reset();

        st.push("a", TestMsg::at(1000)).unwrap();
        st.push("b", TestMsg::at(1005)).unwrap();
        assert!(st.try_match().is_some(), "should sync again after a reset");
    }

    /// M-23, through this pipeline.
    #[test]
    fn reports_why_it_is_not_matching() {
        let mut st = state(50, 8);
        assert_eq!(
            st.match_status().blocked,
            Some(BlockedReason::WaitingForData)
        );

        st.push("a", TestMsg::at(1000)).unwrap();
        assert_eq!(
            st.match_status().blocked,
            Some(BlockedReason::WaitingForData),
            "b has delivered nothing"
        );
    }

    #[test]
    fn reports_topic_count_and_readiness() {
        let mut st = state(50, 8);
        assert_eq!(st.num_topics(), 2);
        assert!(!st.is_ready());

        st.push("a", TestMsg::at(1000)).unwrap();
        st.push("b", TestMsg::at(1005)).unwrap();
        assert!(st.is_ready());
        assert!(!st.is_well_filled(), "only one message per topic");
    }

    #[test]
    fn unknown_topic_is_rejected_and_counted() {
        let mut st = state(50, 8);
        assert!(st.push("nope", TestMsg::at(1000)).is_err());
        assert_eq!(st.stats().dropped, 1);
    }

    #[test]
    fn late_messages_are_counted_separately_from_drops() {
        let mut st = state(50, 8);
        st.push("a", TestMsg::at(5000)).unwrap();
        st.push("b", TestMsg::at(5010)).unwrap();
        st.try_match().unwrap();

        assert!(st.push("a", TestMsg::at(1000)).is_err());
        let stats = st.stats();
        assert_eq!(stats.late_rejected, 1);
        assert_eq!(stats.dropped, 0, "a late message is not an overflow drop");
    }
}
