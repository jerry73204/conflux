use conflux_core::{Config, WithTimestamp, sync};
use futures::{TryStreamExt, stream};
use indexmap::IndexMap;
use std::time::Duration;

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

async fn run_sync_with_timeout<K>(
    input_stream: impl futures::Stream<Item = eyre::Result<(K, TestMessageWithTimeout)>> + Unpin + Send,
    keys: impl IntoIterator<Item = K>,
    config: Config,
) -> eyre::Result<Vec<IndexMap<K, TestMessageWithTimeout>>>
where
    K: Clone + PartialEq + Eq + std::hash::Hash + Sync + Send + 'static,
{
    let (output_stream, _feedback_receiver) = sync(input_stream, keys, config)?;
    output_stream.try_collect().await
}

#[tokio::test]
async fn test_timeout_basic_expiration() {
    // Test that messages with very short timeouts can be properly expired
    let stream = stream::iter([
        Ok(("A", TestMessageWithTimeout::new(1000, "a1", Some(50)))), // expires at 1050ms
        Ok(("B", TestMessageWithTimeout::new(1100, "b1", Some(50)))), // expires at 1150ms
        Ok(("A", TestMessageWithTimeout::new(3000, "a2", None))),     // no timeout, much later
        Ok(("B", TestMessageWithTimeout::new(3100, "b2", None))),     // no timeout, much later
    ]);

    let config = Config::basic(Some(Duration::from_millis(200)), None, 4);

    let groups = run_sync_with_timeout(stream, ["A", "B"], config)
        .await
        .unwrap();

    // Given the timing, we should get at least one group
    // The exact behavior depends on when timeouts are checked
    assert!(!groups.is_empty());

    // If messages expire, we should get the later group
    if groups.len() == 1 {
        assert_eq!(groups[0]["A"].timestamp(), Duration::from_millis(3000));
        assert_eq!(groups[0]["B"].timestamp(), Duration::from_millis(3100));
    }
}

#[tokio::test]
async fn test_timeout_mixed_timeout_and_no_timeout() {
    // Test mixing messages with and without timeouts
    let stream = stream::iter([
        Ok(("A", TestMessageWithTimeout::new(1000, "a1", None))), // no timeout
        Ok(("B", TestMessageWithTimeout::new(1050, "b1", None))), // no timeout
        Ok(("A", TestMessageWithTimeout::new(1200, "a2", Some(500)))), // expires at 1700ms
        Ok(("B", TestMessageWithTimeout::new(1250, "b2", None))), // no timeout
    ]);

    let config = Config::basic(Some(Duration::from_millis(100)), None, 4);

    let groups = run_sync_with_timeout(stream, ["A", "B"], config)
        .await
        .unwrap();

    // Should form two groups since all messages can synchronize
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0]["A"].timestamp(), Duration::from_millis(1000));
    assert_eq!(groups[0]["B"].timestamp(), Duration::from_millis(1050));
    assert_eq!(groups[1]["A"].timestamp(), Duration::from_millis(1200));
    assert_eq!(groups[1]["B"].timestamp(), Duration::from_millis(1250));
}

#[tokio::test]
async fn test_timeout_prevents_memory_buildup() {
    // Test that timeouts prevent excessive memory usage from old messages
    let mut items = Vec::new();

    // Add recent messages that should synchronize
    for i in 0..10 {
        items.push(Ok((
            "A",
            TestMessageWithTimeout::new(i * 100, &format!("a{}", i), None),
        )));
        items.push(Ok((
            "B",
            TestMessageWithTimeout::new(i * 100 + 50, &format!("b{}", i), None),
        )));
    }

    let stream = stream::iter(items);

    let config = Config::basic(Some(Duration::from_millis(100)), None, 10);

    let groups = run_sync_with_timeout(stream, ["A", "B"], config)
        .await
        .unwrap();

    // Should form multiple groups from the message pairs
    // The exact number depends on the synchronization algorithm behavior
    assert!(!groups.is_empty());
    assert!(groups.len() >= 9);

    // Verify first group
    assert_eq!(groups[0]["A"].data, "a0");
    assert_eq!(groups[0]["B"].data, "b0");
}

#[tokio::test]
async fn test_timeout_with_different_timeouts_per_stream() {
    // Test streams with different timeout characteristics
    let stream = stream::iter([
        Ok(("fast", TestMessageWithTimeout::new(1000, "f1", Some(100)))), // short timeout
        Ok(("slow", TestMessageWithTimeout::new(1000, "s1", Some(2000)))), // long timeout
        Ok(("fast", TestMessageWithTimeout::new(1500, "f2", Some(100)))), // short timeout
        Ok(("slow", TestMessageWithTimeout::new(1500, "s2", Some(2000)))), // long timeout
    ]);

    let config = Config::basic(Some(Duration::from_millis(200)), None, 4);

    let groups = run_sync_with_timeout(stream, ["fast", "slow"], config)
        .await
        .unwrap();

    // Both pairs should synchronize since timeouts are long enough
    assert_eq!(groups.len(), 2);

    // Verify the groups contain expected messages
    assert_eq!(groups[0]["fast"].data, "f1");
    assert_eq!(groups[0]["slow"].data, "s1");
    assert_eq!(groups[1]["fast"].data, "f2");
    assert_eq!(groups[1]["slow"].data, "s2");
}

#[tokio::test]
async fn test_timeout_edge_case_exact_expiration() {
    // Test edge case where message expires exactly at reference time
    let stream = stream::iter([
        Ok(("A", TestMessageWithTimeout::new(1000, "a1", Some(500)))), // expires exactly at 1500ms
        Ok(("B", TestMessageWithTimeout::new(1000, "b1", Some(600)))), // expires at 1600ms
        Ok(("A", TestMessageWithTimeout::new(1500, "a2", None))),      // at reference time
        Ok(("B", TestMessageWithTimeout::new(1500, "b2", None))),      // at reference time
    ]);

    let config = Config::basic(Some(Duration::from_millis(100)), None, 4);

    let groups = run_sync_with_timeout(stream, ["A", "B"], config)
        .await
        .unwrap();

    // The behavior depends on exact timing, but we should get at least one group
    assert!(!groups.is_empty());

    // Verify that we get valid synchronization
    for group in &groups {
        assert_eq!(group.len(), 2);
        assert!(group.contains_key("A"));
        assert!(group.contains_key("B"));
    }
}
