use crate::{
    Config, Feedback,
    buffer::Buffer,
    staleness::StalenessDetector,
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
    task::{Context, Poll, Poll::*},
    time::Duration,
};
use tokio::sync::watch;
use tracing::{debug, warn};

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
    // let keys: Vec<_> = keys.into_iter().collect();

    let Config {
        window_size,
        start_time,
        buf_size,
        staleness_config,
    } = config;

    // Sanity check
    ensure!(buf_size >= 2);
    ensure!(window_size > Duration::ZERO);

    // Initialize buffers for respective keys.
    let buffers: IndexMap<_, _> = keys
        .into_iter()
        .map(|key| {
            let buffer = Buffer::with_capacity(buf_size);
            (key, buffer)
        })
        .collect();
    ensure!(!buffers.is_empty());
    // println!("the buffer is shown as below \n {buffers:#?}");

    // Create the queue that pipes generated feedback messages.
    let (feedback_tx, feedback_rx) = {
        let init_feedback = Feedback {
            accepted_max_timestamp: None,
            commit_timestamp: None,
            accepted_keys: buffers.keys().cloned().collect(),
        };
        watch::channel(init_feedback)
    };

    // Initialize staleness detector if configured
    let staleness_detector = staleness_config.map(StalenessDetector::new);

    // Initialize the internal state.
    let mut state = State {
        feedback_tx: Some(feedback_tx),
        buffers,
        commit_ts: start_time,
        buf_size,
        window_size,
        staleness_detector,
    };

    // Construct output stream.
    let output_stream = {
        let mut stream = Some(stream);
        stream::poll_fn(move |ctx| poll(Pin::new(&mut stream), &mut state, ctx))
    };

    Ok((output_stream.boxed(), feedback_rx))
}

/// The polling function is repeated called to generated batched
/// messages.
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
        // println!("......\n{state:#?}\n......");
        // Loop until a valid group is found.
        loop {
            // Clean up expired messages using the latest commit timestamp as reference
            if let Some(commit_ts) = state.commit_ts {
                let _expired_count = state.drop_expired_messages(commit_ts);
            }

            // Process staleness expiration if configured
            let _stale_count = state.process_staleness_expiration();

            // println!("......\n{state:#?}\n......");
            if !state.is_ready() {
                // eprintln!("not ready");
                // Case: Any one of the buffer has one or zero
                // message.

                // Consume one message from the input stream.
                let item = input_stream_mut.as_mut().poll_next(ctx);
                // println!("............\n{:#?}\n",state);
                match item {
                    Ready(Some(Ok(item))) => {
                        let (key, item) = item;
                        let ok = state.push(key, item).is_ok();
                        if !ok {
                            debug!("drop a late message")
                        }
                    } // A message is returned
                    Ready(Some(Err(err))) => {
                        // An error is returned
                        input_stream.set(None);
                        break Some(Err(err));
                    }
                    Ready(None) => {
                        // The input stream is depleted.
                        // input_stream.set(None);
                        // break None;
                        // println!("........\n{:#?}\n........",state);
                        if !state.is_empty() {
                            // println!("checking the buffers still have datas");
                            if let Some(matching) = state.try_match() {
                                state.update_feedback();
                                // println!("when input stream is depeleted and there are still matching");
                                input_stream.set(None);
                                break Some(Ok(matching));
                            } else {
                                // println!("there are still datas, but matching has failed");
                                // input_stream.set(None);
                                // break None;
                                state.drop_min();
                                continue;
                            }
                        } else {
                            input_stream.set(None);
                            break None;
                        }
                    }
                    Pending => {
                        // The input stream is not ready.
                        return Pending;
                    }
                };

                // Try to insert the message.
                // let ok = state.push(key, item).is_ok();
                // state.update_feedback();

                // If failed, tell the input stream to catch up and
                // retry.
                // if !ok {
                //     debug!("drop a late message");
                // }
            } else if state.is_full() {
                // eprintln!("full");
                // Case: All buffers are full.

                // Try to group up messages. If successful, return the
                // group. Otherwise, drop the message with minimum
                // timestamp and retry.
                if let Some(matching) = state.try_match() {
                    state.update_feedback();
                    break Some(Ok(matching));
                } else {
                    warn!(
                        "Unable to find a new matching while all buffers are full.\
                         Drop one message anyway."
                    );
                    state.drop_min();
                    state.update_feedback();
                }
            } else {
                // eprintln!("ready");
                // Case: All buffers have at least 2 messages and not
                // all buffers are full.

                // Consume a message from the input stream.
                let item = input_stream_mut.as_mut().poll_next(ctx);

                match item {
                    Ready(Some(Ok(item))) => {
                        let (_key, _item) = item;
                        if state.push(_key, _item).is_err() {
                            state.update_feedback();
                            continue;
                        }
                    }
                    Ready(Some(Err(err))) => {
                        input_stream.set(None);
                        break Some(Err(err));
                    }
                    Ready(None) => {
                        // println!("input stream has been depleted.............");
                        // input_stream.set(None);
                        // break None; // TODO
                        if let Some(matching) = state.try_match() {
                            state.update_feedback();
                            input_stream.set(None);
                            break Some(Ok(matching));
                        } else {
                            state.drop_min();
                            continue;
                            // input_stream.set(None);
                            // break None;
                        }
                    }
                    Pending => {
                        return Pending;
                    }
                };

                // Try to insert the message to one of the buffer.  If
                // not successful, emit a feedback to tell the input
                // stream to catch up.
                // if state.push(key, item).is_err() {
                //     // debug!("drop a late message for device {:?}", device);
                //     state.update_feedback();
                //     continue;
                // }

                // Try to group up messages.
                let matching = state.try_match();

                // Emit a feedback.
                state.update_feedback();

                // Emit the group if a group is successfully formed.
                if let Some(matching) = matching {
                    break Some(Ok(matching));
                }
            }
        }
    } else {
        // eprintln!("depleted");
        // Case: the input stream is depleted.
        // Loop until a valid group is found.
        loop {
            // Clean up expired messages using the latest commit timestamp as reference
            if let Some(commit_ts) = state.commit_ts {
                let _expired_count = state.drop_expired_messages(commit_ts);
            }

            if state.is_empty() {
                break None;
            } else if let Some(matching) = state.try_match() {
                break Some(Ok(matching));
            } else {
                // println!("......\n{state:#?}\n......");
                state.drop_min();
            }
        }
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
        let config = Config {
            window_size: Duration::from_millis(100),
            start_time: None,
            buf_size: 4,
            staleness_config: None,
        };

        let empty_stream = stream::empty::<eyre::Result<(&str, TestMessage)>>();
        let keys = ["A", "B"];

        let result = sync(empty_stream, keys, config);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_config_buf_size_too_small() {
        let config = Config {
            window_size: Duration::from_millis(100),
            start_time: None,
            buf_size: 1,
            staleness_config: None,
        };

        let empty_stream = stream::empty::<eyre::Result<(&str, TestMessage)>>();
        let keys = ["A", "B"];

        let result = sync(empty_stream, keys, config);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_config_window_size_zero() {
        let config = Config {
            window_size: Duration::ZERO,
            start_time: None,
            buf_size: 4,
            staleness_config: None,
        };

        let empty_stream = stream::empty::<eyre::Result<(&str, TestMessage)>>();
        let keys = ["A", "B"];

        let result = sync(empty_stream, keys, config);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_config_empty_key_list() {
        let config = Config {
            window_size: Duration::from_millis(100),
            start_time: None,
            buf_size: 4,
            staleness_config: None,
        };

        let empty_stream = stream::empty::<eyre::Result<(&str, TestMessage)>>();
        let keys: Vec<&str> = vec![];

        let result = sync(empty_stream, keys, config);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_config_minimum_valid_values() {
        let config = Config {
            window_size: Duration::from_nanos(1), // Minimum valid window
            start_time: None,
            buf_size: 2, // Minimum valid buffer size
            staleness_config: None,
        };

        let empty_stream = stream::empty::<eyre::Result<(&str, TestMessage)>>();
        let keys = ["A"];

        let result = sync(empty_stream, keys, config);
        assert!(result.is_ok());
    }
}
