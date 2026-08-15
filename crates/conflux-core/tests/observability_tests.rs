//! M-23: a synchronizer that has stopped emitting must be able to say why.
//!
//! The exported statistics only ever described inputs -- received, rejected,
//! buffer length. None of them distinguish "waiting for the next message" from
//! "wedged and never emitting again", which is what made C-05 expensive to
//! diagnose in the first place.

use conflux_core::{BlockedReason, DropPolicy, WithTimestamp, buffer::Buffer, state::State};
use indexmap::IndexMap;
use std::{sync::Arc, time::Duration};
use tokio::sync::Notify;

#[derive(Debug, Clone, PartialEq, Eq)]
struct M(Duration);

impl WithTimestamp for M {
    fn timestamp(&self) -> Duration {
        self.0
    }
}

fn ms(v: u64) -> M {
    M(Duration::from_millis(v))
}

fn state(buf: usize, window_ms: u64) -> State<&'static str, M> {
    let mut buffers = IndexMap::new();
    buffers.insert("A", Buffer::with_capacity(buf));
    buffers.insert("B", Buffer::with_capacity(buf));
    State {
        buffers,
        commit_ts: None,
        buf_size: buf,
        window_size: Some(Duration::from_millis(window_ms)),
        drop_policy: DropPolicy::RejectNew,
        feedback_tx: None,
        space_notify: Arc::new(Notify::new()),
    }
}

#[test]
fn reports_waiting_for_data_when_a_stream_is_silent() {
    let mut st = state(8, 50);
    st.push("A", ms(1000)).unwrap();
    st.push("A", ms(1010)).unwrap();

    let status = st.match_status();
    assert_eq!(status.blocked, Some(BlockedReason::WaitingForData));
    assert_eq!(
        status.spread, None,
        "no spread is defined while a stream is empty"
    );
}

#[test]
fn reports_spread_too_narrow_with_the_shortfall() {
    let mut st = state(8, 50);
    // Both streams have data, but everything sits inside a 20ms band, so
    // `try_match` holds out for more.
    st.push("A", ms(1000)).unwrap();
    st.push("A", ms(1020)).unwrap();
    st.push("B", ms(1005)).unwrap();
    st.push("B", ms(1015)).unwrap();

    let status = st.match_status();
    assert_eq!(status.blocked, Some(BlockedReason::SpreadTooNarrow));
    assert_eq!(status.inf_ts, Some(Duration::from_millis(1005)));
    assert_eq!(status.sup_ts, Some(Duration::from_millis(1015)));
    assert_eq!(
        status.shortfall,
        Some(Duration::from_millis(40)),
        "needs inf+window-sup = 1005+50-1015 = 40ms more spread"
    );
}

#[test]
fn reports_nothing_blocked_when_a_group_is_available() {
    let mut st = state(8, 50);
    st.push("A", ms(1000)).unwrap();
    st.push("B", ms(1005)).unwrap();

    // A single message each: `all_one` lets this match immediately.
    assert_eq!(st.match_status().blocked, None);
}

#[test]
fn reports_buffer_full_no_match_in_the_c05_wedge_state() {
    let mut st = state(2, 50);
    // The C-05 shape: buffers full, streams far enough apart that nothing pairs.
    st.push("A", ms(1000)).unwrap();
    st.push("A", ms(1010)).unwrap();
    st.push("B", ms(5000)).unwrap();
    st.push("B", ms(5010)).unwrap();

    let status = st.match_status();
    assert_eq!(status.blocked, Some(BlockedReason::BufferFullNoMatch));
}

#[test]
fn match_status_does_not_mutate_the_state() {
    let mut st = state(8, 50);
    st.push("A", ms(1000)).unwrap();
    st.push("A", ms(1020)).unwrap();
    st.push("B", ms(1005)).unwrap();

    let before = (st.buffers["A"].len(), st.buffers["B"].len());
    let _ = st.match_status();
    let after = (st.buffers["A"].len(), st.buffers["B"].len());

    assert_eq!(before, after, "inspecting status must not consume messages");
}
