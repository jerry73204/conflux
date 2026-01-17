use eyre::Result;
use futures::stream::BoxStream;
use indexmap::IndexMap;
use std::{hash::Hash, time::Duration};
use tokio::sync::watch;

/// Creates a timestamp from the message passed to the synchronizer.
pub trait WithTimestamp: Send {
    fn timestamp(&self) -> Duration;

    /// Optional timeout for how long the message can stay in buffer.
    /// Returns None for no timeout (default behavior).
    fn timeout(&self) -> Option<Duration> {
        None
    }
}

/// The key that identifies the queue in the synchronizer.
pub trait Key: Clone + PartialEq + Eq + Hash + Sync + Send {}

impl<K> Key for K where K: Clone + PartialEq + Eq + Hash + Sync + Send {}

/// The feedback message generated from [sync](crate::sync()) to control
/// the pace of input streams.
#[derive(Debug, Clone)]
pub struct Feedback<K>
where
    K: Key,
{
    pub accepted_max_timestamp: Option<Duration>,
    pub commit_timestamp: Option<Duration>,
    pub accepted_keys: Vec<K>,
}

/// The stream is returned by [sync](crate::sync()), emitting batches of
/// messages within a time window.
pub type OutputStream<'a, K, T> = BoxStream<'a, Result<IndexMap<K, T>>>;

/// The stream is returned by [sync](crate::sync()) to control the pace
/// of input stream.
pub type FeedbackReceiver<K> = watch::Receiver<Feedback<K>>;
