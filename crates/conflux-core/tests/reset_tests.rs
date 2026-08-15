//! M-22: a stream whose clock goes backwards must be recoverable.
//!
//! `Buffer` keeps a monotonic high-water mark and rejects anything at or below
//! it. That is correct within a monotonic stream, but a bag loop, a sim-time
//! reset, or a reconnecting sensor legitimately restarts its stamps -- and
//! without a way to clear the mark, the affected buffer rejects every later
//! message forever, stalling the whole synchronizer.

use conflux_core::{
    DropPolicy, WithTimestamp,
    buffer::Buffer,
    state::{PushError, State},
};
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

fn state(buf: usize) -> State<&'static str, M> {
    let mut buffers = IndexMap::new();
    buffers.insert("A", Buffer::with_capacity(buf));
    buffers.insert("B", Buffer::with_capacity(buf));
    State {
        buffers,
        commit_ts: None,
        buf_size: buf,
        window_size: Some(Duration::from_millis(50)),
        drop_policy: DropPolicy::RejectNew,
        feedback_tx: None,
        space_notify: Arc::new(Notify::new()),
    }
}

#[test]
fn buffer_reset_clears_the_monotonic_high_water_mark() {
    let mut buffer: Buffer<M> = Buffer::with_capacity(4);
    buffer.try_push(ms(5000)).unwrap();
    assert!(
        buffer.try_push(ms(1000)).is_err(),
        "backwards push is rejected"
    );

    buffer.reset();

    assert_eq!(buffer.len(), 0, "reset should drop buffered messages");
    assert_eq!(
        buffer.last_ts(),
        None,
        "reset should clear the high-water mark"
    );
    assert!(
        buffer.try_push(ms(1000)).is_ok(),
        "after reset the stream may restart from an earlier timestamp"
    );
}

#[test]
fn state_reset_revives_a_stream_after_a_clock_jump() {
    let mut st = state(8);

    // Normal operation, then a group is emitted so commit_ts advances.
    st.push("A", ms(5000)).unwrap();
    st.push("B", ms(5010)).unwrap();
    assert!(st.advance().is_some(), "aligned pair should match");

    // The source restarts its clock. Every stream is now dead.
    assert!(matches!(
        st.push("A", ms(1000)),
        Err(PushError::LateMessage(_)) | Err(PushError::OutOfOrder(_))
    ));

    st.reset();

    st.push("A", ms(1000))
        .expect("A should accept the restarted clock");
    st.push("B", ms(1010))
        .expect("B should accept the restarted clock");
    assert!(
        st.advance().is_some(),
        "synchronizer should produce groups again after a reset"
    );
}

#[test]
fn state_reset_clears_the_commit_timestamp() {
    let mut st = state(8);
    st.push("A", ms(5000)).unwrap();
    st.push("B", ms(5010)).unwrap();
    st.advance().unwrap();
    assert!(
        st.commit_ts.is_some(),
        "matching should set a commit timestamp"
    );

    st.reset();

    assert_eq!(
        st.commit_ts, None,
        "a stale commit timestamp would keep rejecting the restarted stream"
    );
}
