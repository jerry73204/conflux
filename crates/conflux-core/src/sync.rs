use crate::{
    Config, Feedback,
    buffer::Buffer,
    state::State,
    types::{FeedbackReceiver, Key, OutputStream, WithTimestamp},
};
use eyre::{Result, ensure};
use futures::{
    self, StreamExt,
    stream::{self, Stream},
};
use indexmap::IndexMap;
use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Poll::*},
    time::Duration,
};
use tokio::sync::{Notify, watch};
use tracing::debug;

/// Consume a stream of messages, each identified by a key, and group
/// up messages within a time window with distinct keys.
///
/// The function returns an output stream and a feedback stream. The
/// output stream emits batches of grouped messages. The feedback
/// stream emits feedback messages to control the input stream.
pub fn sync<'a, K, T, S, I>(
    stream: S,
    keys: I,
    config: Config,
) -> Result<(OutputStream<'a, K, T>, FeedbackReceiver<K>)>
where
    K: Key + 'a,
    T: WithTimestamp + Clone + 'a,
    S: Stream<Item = Result<(K, T)>> + Unpin + Send + 'a,
    I: IntoIterator<Item = K>,
{
    let Config {
        window_size,
        start_time,
        buf_size,
        drop_policy,
    } = config;

    // L-21: state the constraint and the reason. The floor is 2 because the
    // matcher needs room to hold a second message per stream while deciding
    // whether a better pairing is still coming.
    ensure!(
        buf_size >= 2,
        "buf_size must be at least 2, got {buf_size}: the matcher needs room for \
         a second message per stream to compare candidate pairings"
    );

    // L-20: `None` means an infinite window. Zero is not a synonym for it -- zero
    // is the natural spelling of "no tolerance at all" -- so reject it explicitly
    // rather than let it read as a tightening that silently disables windowing.
    if let Some(ws) = window_size {
        ensure!(
            ws > Duration::ZERO,
            "window_size must be positive; pass None for an infinite window \
             (no time-based dropping) rather than zero"
        );
    }

    // Initialize buffers for respective keys.
    let buffers: IndexMap<_, _> = keys
        .into_iter()
        .map(|key| {
            let buffer = Buffer::with_capacity(buf_size);
            (key, buffer)
        })
        .collect();
    ensure!(!buffers.is_empty());

    // Create the queue that pipes generated feedback messages.
    let (feedback_tx, feedback_rx) = {
        let init_feedback = Feedback {
            commit_timestamp: None,
            accepted_keys: buffers.keys().cloned().collect(),
        };
        watch::channel(init_feedback)
    };

    // Initialize the internal state.
    let mut state = State {
        feedback_tx: Some(feedback_tx),
        buffers,
        commit_ts: start_time,
        buf_size,
        window_size,
        drop_policy,
        space_notify: Arc::new(Notify::new()),
    };

    // Construct output stream.
    let output_stream = {
        let mut stream = Some(stream);
        stream::poll_fn(move |ctx| poll(Pin::new(&mut stream), &mut state, ctx))
    };

    Ok((output_stream.boxed(), feedback_rx))
}

/// Drain whatever remains once no more input will arrive.
///
/// `advance` already forces progress when a buffer is full, but at end of input
/// there is nothing left to wait for at all, so keep dropping the oldest message
/// until a group forms or a stream runs dry.
fn drain<K, T>(state: &mut State<K, T>) -> Option<Result<IndexMap<K, T>>>
where
    K: Key,
    T: WithTimestamp + Clone,
{
    loop {
        // No group can be formed while any stream is missing data.
        if state.has_empty_buffer() {
            return None;
        }
        if let Some(matching) = state.advance() {
            return Some(Ok(matching));
        }
        if !state.drop_min() {
            return None;
        }
    }
}

/// The polling function is repeated called to generated batched
/// messages.
///
/// H-12: the matching rules live in [`State::advance`], shared with the C FFI
/// driver. This function is only responsible for feeding the state from the
/// input stream and for end-of-input draining -- it no longer carries its own
/// copy of the match/drop policy, which is how the two pipelines drifted apart.
fn poll<K, T, S>(
    mut input_stream: Pin<&mut Option<S>>,
    state: &mut State<K, T>,
    ctx: &mut Context<'_>,
) -> Poll<Option<Result<IndexMap<K, T>>>>
where
    K: Key,
    S: Stream<Item = Result<(K, T)>> + Unpin + Send,
    T: WithTimestamp + Clone + Send,
{
    let group = if let Some(mut input_stream_mut) = input_stream.as_mut().as_pin_mut() {
        // Case: the input stream is not depleted yet.
        loop {
            // Clean up expired messages using the latest commit timestamp as reference.
            if let Some(commit_ts) = state.commit_ts {
                let _expired_count = state.drop_expired_messages(commit_ts);
            }

            // Emit as soon as a group is available. `advance` also forces
            // progress when a buffer is full and nothing can match, so this
            // cannot spin forever waiting on a stream that can no longer grow.
            if let Some(matching) = state.advance() {
                state.update_feedback();
                break Some(Ok(matching));
            }

            // No group yet -- take another message from the input.
            match input_stream_mut.as_mut().poll_next(ctx) {
                Ready(Some(Ok((key, item)))) => {
                    if state.push(key, item).is_err() {
                        // Late, out-of-order, or rejected by the drop policy.
                        debug!("dropped a rejected message");
                        state.update_feedback();
                    }
                }
                Ready(Some(Err(err))) => {
                    input_stream.set(None);
                    break Some(Err(err));
                }
                Ready(None) => {
                    // Input exhausted: drain whatever is still buffered.
                    let drained = drain(state);
                    state.update_feedback();
                    input_stream.set(None);
                    break drained;
                }
                Pending => {
                    // The input stream is not ready.
                    return Pending;
                }
            }
        }
    } else {
        // Case: the input stream is depleted.
        // Clean up expired messages using the latest commit timestamp as reference.
        if let Some(commit_ts) = state.commit_ts {
            let _expired_count = state.drop_expired_messages(commit_ts);
        }

        drain(state)
    };

    Ready(group)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Config, WithTimestamp};
    use futures::stream;
    use std::time::Duration;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestMessage {
        pub timestamp: Duration,
        pub data: String,
    }

    impl WithTimestamp for TestMessage {
        fn timestamp(&self) -> Duration {
            self.timestamp
        }
    }

    #[tokio::test]
    async fn test_config_valid_configuration() {
        let config = Config::basic(Some(Duration::from_millis(100)), None, 4);

        let empty_stream = stream::empty::<eyre::Result<(&str, TestMessage)>>();
        let keys = ["A", "B"];

        let result = sync(empty_stream, keys, config);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_config_buf_size_too_small() {
        let config = Config::basic(Some(Duration::from_millis(100)), None, 1);

        let empty_stream = stream::empty::<eyre::Result<(&str, TestMessage)>>();
        let keys = ["A", "B"];

        let result = sync(empty_stream, keys, config);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_config_window_size_zero() {
        let config = Config::basic(Some(Duration::ZERO), None, 4);

        let empty_stream = stream::empty::<eyre::Result<(&str, TestMessage)>>();
        let keys = ["A", "B"];

        let result = sync(empty_stream, keys, config);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_config_empty_key_list() {
        let config = Config::basic(Some(Duration::from_millis(100)), None, 4);

        let empty_stream = stream::empty::<eyre::Result<(&str, TestMessage)>>();
        let keys: Vec<&str> = vec![];

        let result = sync(empty_stream, keys, config);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_config_minimum_valid_values() {
        let config = Config::basic(Some(Duration::from_nanos(1)), None, 2);

        let empty_stream = stream::empty::<eyre::Result<(&str, TestMessage)>>();
        let keys = ["A"];

        let result = sync(empty_stream, keys, config);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_config_infinite_window() {
        // Infinite window (None) should be valid
        let config = Config::offline(4);

        let empty_stream = stream::empty::<eyre::Result<(&str, TestMessage)>>();
        let keys = ["A", "B"];

        let result = sync(empty_stream, keys, config);
        assert!(result.is_ok());
    }
}
