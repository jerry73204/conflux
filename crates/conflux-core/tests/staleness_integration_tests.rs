use conflux_core::{Config, StalenessConfig, WithTimestamp, sync};
use futures::{TryStreamExt, stream};
use indexmap::IndexMap;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
struct TestMessage {
    timestamp: Duration,
    data: String,
}

impl TestMessage {
    fn new(timestamp_ms: u64, data: &str) -> Self {
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

async fn run_sync_with_staleness<'a>(
    messages: Vec<(&'a str, TestMessage)>,
    keys: Vec<&'a str>,
    config: Config,
) -> eyre::Result<Vec<IndexMap<&'a str, TestMessage>>> {
    let stream = stream::iter(messages.into_iter().map(Ok));
    let (output_stream, _feedback_receiver) = sync(stream, keys, config)?;
    output_stream.try_collect().await
}

#[tokio::test]
async fn test_staleness_integration_basic() {
    // Test basic staleness functionality with simple message flow
    let messages = vec![
        ("A", TestMessage::new(1000, "a1")),
        ("B", TestMessage::new(1050, "b1")),
        ("A", TestMessage::new(2000, "a2")),
        ("B", TestMessage::new(2050, "b2")),
    ];

    let staleness_config = StalenessConfig::default();
    let config = Config::with_staleness(Duration::from_millis(100), None, 16, staleness_config);

    let groups = run_sync_with_staleness(messages, vec!["A", "B"], config)
        .await
        .unwrap();

    // Should form two groups as normal
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0]["A"].data, "a1");
    assert_eq!(groups[0]["B"].data, "b1");
    assert_eq!(groups[1]["A"].data, "a2");
    assert_eq!(groups[1]["B"].data, "b2");
}

#[tokio::test]
async fn test_staleness_integration_with_lazy_checking() {
    // Test staleness with immediate expiration disabled (lazy checking)
    let messages = vec![
        ("A", TestMessage::new(1000, "a1")),
        ("B", TestMessage::new(1050, "b1")),
        ("A", TestMessage::new(2000, "a2")),
        ("B", TestMessage::new(2050, "b2")),
    ];

    let staleness_config = StalenessConfig {
        enable_immediate_expiration: false, // Use lazy checking
        ..StalenessConfig::default()
    };

    let config = Config::with_staleness(Duration::from_millis(100), None, 16, staleness_config);

    let groups = run_sync_with_staleness(messages, vec!["A", "B"], config)
        .await
        .unwrap();

    // Should still work with lazy checking
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0]["A"].data, "a1");
    assert_eq!(groups[0]["B"].data, "b1");
}

#[tokio::test]
async fn test_staleness_integration_high_frequency() {
    // Test high-frequency staleness configuration
    let mut messages = vec![];

    // Generate rapid message stream
    for i in 0..20 {
        messages.push(("A", TestMessage::new(i * 10, &format!("a{}", i))));
        messages.push(("B", TestMessage::new(i * 10 + 5, &format!("b{}", i))));
    }

    let staleness_config = StalenessConfig {
        enable_immediate_expiration: false, // Disable for testing without tokio feature
        ..StalenessConfig::high_frequency()
    };
    let config = Config::with_staleness(Duration::from_millis(20), None, 32, staleness_config);

    let groups = run_sync_with_staleness(messages, vec!["A", "B"], config)
        .await
        .unwrap();

    // Should handle high-frequency messages efficiently
    assert!(!groups.is_empty());
    assert!(groups.len() >= 15); // Most messages should synchronize
}

#[tokio::test]
async fn test_staleness_integration_batch_processing() {
    // Test batch processing configuration
    let messages = vec![
        ("A", TestMessage::new(1000, "a1")),
        ("B", TestMessage::new(1100, "b1")),
        ("C", TestMessage::new(1200, "c1")),
        ("A", TestMessage::new(3000, "a2")),
        ("B", TestMessage::new(3100, "b2")),
        ("C", TestMessage::new(3200, "c2")),
    ];

    let staleness_config = StalenessConfig::batch_processing();
    let config = Config::with_staleness(Duration::from_millis(500), None, 16, staleness_config);

    let groups = run_sync_with_staleness(messages, vec!["A", "B", "C"], config)
        .await
        .unwrap();

    // Should form groups despite relaxed timing
    assert!(!groups.is_empty());

    // Verify each group has the expected keys
    for group in &groups {
        assert!(group.contains_key("A"));
        assert!(group.contains_key("B"));
        assert!(group.contains_key("C"));
    }
}

#[tokio::test]
async fn test_staleness_integration_mixed_configurations() {
    // Test with mixed window sizes and staleness configurations
    let messages = vec![
        ("fast", TestMessage::new(1000, "f1")),
        ("slow", TestMessage::new(1000, "s1")),
        ("fast", TestMessage::new(1100, "f2")),
        ("slow", TestMessage::new(1200, "s2")),
        ("fast", TestMessage::new(1300, "f3")),
        ("slow", TestMessage::new(1400, "s3")),
    ];

    let staleness_config = StalenessConfig {
        enable_immediate_expiration: false, // Disable for testing without tokio feature
        ..StalenessConfig::low_frequency()
    };
    let config = Config::with_staleness(
        Duration::from_millis(300), // Large window
        None,
        16,
        staleness_config,
    );

    let groups = run_sync_with_staleness(messages, vec!["fast", "slow"], config)
        .await
        .unwrap();

    // Should synchronize well with large window
    assert!(!groups.is_empty());

    for group in &groups {
        assert_eq!(group.len(), 2); // Both keys should be present
        assert!(group.contains_key("fast"));
        assert!(group.contains_key("slow"));
    }
}

#[tokio::test]
async fn test_staleness_integration_config_builder() {
    // Test the config builder methods
    let messages = vec![
        ("A", TestMessage::new(1000, "a1")),
        ("B", TestMessage::new(1050, "b1")),
    ];

    // Test basic config
    let basic_config = Config::basic(Duration::from_millis(100), None, 16);
    let groups_basic = run_sync_with_staleness(messages.clone(), vec!["A", "B"], basic_config)
        .await
        .unwrap();

    // Test with staleness enabled
    let staleness_config = Config::basic(Duration::from_millis(100), None, 16)
        .enable_staleness(StalenessConfig::default());
    let groups_staleness = run_sync_with_staleness(messages, vec!["A", "B"], staleness_config)
        .await
        .unwrap();

    // Both should produce same results for this simple case
    assert_eq!(groups_basic.len(), groups_staleness.len());
    assert_eq!(groups_basic[0]["A"].data, groups_staleness[0]["A"].data);
    assert_eq!(groups_basic[0]["B"].data, groups_staleness[0]["B"].data);
}

#[tokio::test]
async fn test_staleness_integration_memory_efficiency() {
    // Test that staleness detection helps with memory efficiency
    let mut messages = vec![];

    // Create a scenario where one stream stops sending messages
    for i in 0..10 {
        messages.push(("A", TestMessage::new(i * 100, &format!("a{}", i))));
        if i < 5 {
            // Stream B stops early
            messages.push(("B", TestMessage::new(i * 100 + 50, &format!("b{}", i))));
        }
    }

    let staleness_config = StalenessConfig {
        heap_max_size: 32,
        heap_time_horizon: Duration::from_millis(200),
        precision_gap: Duration::from_millis(10),
        timer_wheel_slots: 16,
        timer_wheel_slot_duration: Duration::from_millis(50),
        enable_immediate_expiration: false, // Use lazy checking for testing
    };

    let config = Config::with_staleness(Duration::from_millis(150), None, 16, staleness_config);

    let groups = run_sync_with_staleness(messages, vec!["A", "B"], config)
        .await
        .unwrap();

    // Should form some groups from the early messages
    assert!(!groups.is_empty());

    // Should have synchronized the early A and B messages
    let early_groups: Vec<_> = groups
        .iter()
        .filter(|g| g.contains_key("A") && g.contains_key("B"))
        .collect();

    assert!(!early_groups.is_empty());
}

#[tokio::test]
async fn test_staleness_integration_overflow_to_timer_wheel() {
    // Test that messages properly overflow from heap to timer wheel
    let mut messages = vec![];

    // Generate many messages to overflow the heap
    for i in 0..100 {
        messages.push(("A", TestMessage::new(i * 10, &format!("a{}", i))));
        messages.push(("B", TestMessage::new(i * 10 + 5, &format!("b{}", i))));
    }

    let staleness_config = StalenessConfig {
        heap_max_size: 4, // Very small heap to force overflow
        heap_time_horizon: Duration::from_millis(50),
        precision_gap: Duration::from_micros(100),
        timer_wheel_slots: 32,
        timer_wheel_slot_duration: Duration::from_millis(10),
        enable_immediate_expiration: false,
    };

    let config = Config::with_staleness(Duration::from_millis(25), None, 32, staleness_config);

    let groups = run_sync_with_staleness(messages, vec!["A", "B"], config)
        .await
        .unwrap();

    // Should still process messages despite small heap
    assert!(!groups.is_empty());
    assert!(groups.len() >= 50); // Most should still synchronize
}

#[tokio::test]
async fn test_staleness_integration_precision_gap_coalescing() {
    // Test precision gap coalescing behavior
    let messages = vec![
        ("A", TestMessage::new(1000, "a1")),
        ("B", TestMessage::new(1000, "b1")),
        ("A", TestMessage::new(1001, "a2")), // Very close timing
        ("B", TestMessage::new(1001, "b2")), // Should coalesce
        ("A", TestMessage::new(2000, "a3")),
        ("B", TestMessage::new(2000, "b3")),
    ];

    let staleness_config = StalenessConfig {
        precision_gap: Duration::from_millis(5), // Large gap for coalescing
        ..StalenessConfig::default()
    };

    let config = Config::with_staleness(Duration::from_millis(50), None, 16, staleness_config);

    let groups = run_sync_with_staleness(messages, vec!["A", "B"], config)
        .await
        .unwrap();

    // Should efficiently handle close-timing messages
    assert!(!groups.is_empty());

    // Verify we get proper synchronization
    for group in &groups {
        assert_eq!(group.len(), 2);
        assert!(group.contains_key("A"));
        assert!(group.contains_key("B"));
    }
}
