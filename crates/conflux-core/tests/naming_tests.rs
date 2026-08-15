//! L-17: `is_empty()` reads as "the synchronizer is empty" but means "at least
//! one buffer is empty" -- the opposite guard for the multi-stream case that is
//! the only case conflux exists to handle.

use conflux_core::{DropPolicy, WithTimestamp, buffer::Buffer, state::State};
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

fn state() -> State<&'static str, M> {
    let mut buffers = IndexMap::new();
    buffers.insert("A", Buffer::with_capacity(8));
    buffers.insert("B", Buffer::with_capacity(8));
    State {
        buffers,
        commit_ts: None,
        buf_size: 8,
        window_size: Some(Duration::from_millis(50)),
        drop_policy: DropPolicy::RejectNew,
        feedback_tx: None,
        space_notify: Arc::new(Notify::new()),
    }
}

#[test]
fn has_empty_buffer_says_what_it_means() {
    let mut st = state();
    assert!(st.has_empty_buffer(), "both buffers start empty");

    st.push("A", M(Duration::from_millis(1000))).unwrap();
    assert!(
        st.has_empty_buffer(),
        "B is still empty, so no group can form"
    );

    st.push("B", M(Duration::from_millis(1005))).unwrap();
    assert!(!st.has_empty_buffer(), "every stream now has data");
}

#[test]
fn all_buffers_empty_is_the_predicate_the_old_name_implied() {
    let mut st = state();
    assert!(st.all_buffers_empty(), "nothing pushed yet");

    st.push("A", M(Duration::from_millis(1000))).unwrap();
    assert!(
        !st.all_buffers_empty(),
        "A holds a message, so the state is not wholly empty"
    );
    // ...while the any-empty predicate still reports true. This is exactly the
    // distinction the single `is_empty` name collapsed.
    assert!(st.has_empty_buffer());
}

#[test]
#[allow(deprecated)]
fn deprecated_is_empty_alias_still_works() {
    let mut st = state();
    st.push("A", M(Duration::from_millis(1000))).unwrap();
    assert_eq!(st.is_empty(), st.has_empty_buffer());
}
