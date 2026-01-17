use conflux_core::{Config, WithTimestamp, sync};
use futures::{
    Stream,
    stream::{self, TryStreamExt},
};
use indexmap::IndexMap;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestMessage {
    pub timestamp: Duration,
    pub data: String,
}

impl TestMessage {
    pub fn new(timestamp_ms: u64, data: &str) -> Self {
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

pub fn create_message(timestamp_ms: u64) -> TestMessage {
    TestMessage::new(timestamp_ms, &format!("msg_{}", timestamp_ms))
}

#[allow(dead_code)]
pub fn create_messages(timestamps_ms: &[u64]) -> Vec<TestMessage> {
    timestamps_ms.iter().map(|&ts| create_message(ts)).collect()
}

/// StreamBuilder for creating test streams with various characteristics
pub struct StreamBuilder<K> {
    messages: Vec<(K, TestMessage)>,
}

impl<K> Default for StreamBuilder<K>
where
    K: Clone,
{
    fn default() -> Self {
        Self {
            messages: Vec::new(),
        }
    }
}

impl<K> StreamBuilder<K>
where
    K: Clone,
{
    pub fn new() -> Self {
        Self::default()
    }

    #[allow(dead_code)]
    pub fn add_message(mut self, key: K, timestamp_ms: u64) -> Self {
        self.messages.push((key, create_message(timestamp_ms)));
        self
    }

    pub fn add_messages(mut self, key: K, timestamps_ms: &[u64]) -> Self {
        for &ts in timestamps_ms {
            self.messages.push((key.clone(), create_message(ts)));
        }
        self
    }

    pub fn build(self) -> impl Stream<Item = eyre::Result<(K, TestMessage)>> {
        stream::iter(self.messages.into_iter().map(Ok))
    }
}

/// Helper function to run synchronizer and collect all groups
pub async fn run_sync<K>(
    input_stream: impl Stream<Item = eyre::Result<(K, TestMessage)>> + Unpin + Send,
    keys: impl IntoIterator<Item = K>,
    config: Config,
) -> eyre::Result<Vec<IndexMap<K, TestMessage>>>
where
    K: Clone + PartialEq + Eq + std::hash::Hash + Sync + Send,
{
    let (output_stream, _feedback) = sync(input_stream, keys, config)?;
    let groups: Vec<IndexMap<K, TestMessage>> = output_stream.try_collect().await?;
    Ok(groups)
}

/// Assert that groups contain expected messages with proper ordering
pub fn assert_groups_valid(groups: &[IndexMap<&str, TestMessage>], window_size: Duration) {
    for group in groups {
        assert!(!group.is_empty(), "Group should not be empty");

        // Check that all messages in group are within window size
        let timestamps: Vec<Duration> = group.values().map(|msg| msg.timestamp()).collect();
        let min_ts = timestamps.iter().min().unwrap();
        let max_ts = timestamps.iter().max().unwrap();

        assert!(
            *max_ts - *min_ts <= window_size,
            "Messages in group exceed window size: min={:?}, max={:?}, window={:?}",
            min_ts,
            max_ts,
            window_size
        );
    }
}

/// Assert that groups are in timestamp order
pub fn assert_timestamp_ordering(groups: &[IndexMap<&str, TestMessage>]) {
    let mut prev_min_ts: Option<Duration> = None;

    for group in groups {
        let min_ts = group.values().map(|msg| msg.timestamp()).min().unwrap();

        if let Some(prev) = prev_min_ts {
            assert!(
                min_ts >= prev,
                "Groups not in timestamp order: prev={:?}, current={:?}",
                prev,
                min_ts
            );
        }

        prev_min_ts = Some(min_ts);
    }
}

/// Create a standard config for testing
#[allow(dead_code)]
pub fn default_config() -> Config {
    Config {
        window_size: Duration::from_millis(100),
        start_time: None,
        buf_size: 16,
        staleness_config: None,
    }
}

/// Create a config with custom window size
pub fn config_with_window(window_ms: u64) -> Config {
    Config {
        window_size: Duration::from_millis(window_ms),
        start_time: None,
        buf_size: 16,
        staleness_config: None,
    }
}

/// Create a config with custom buffer size
#[allow(dead_code)]
pub fn config_with_buffer_size(buf_size: usize) -> Config {
    Config {
        window_size: Duration::from_millis(100),
        start_time: None,
        buf_size,
        staleness_config: None,
    }
}
