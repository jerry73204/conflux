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

#[tokio::test]
async fn test_staleness_config_creation() {
    // Test that various staleness configurations can be created
    let default_config = StalenessConfig::default();
    assert!(!default_config.enable_immediate_expiration);
    assert_eq!(default_config.heap_max_size, 256);

    let high_freq_config = StalenessConfig::high_frequency();
    assert_eq!(high_freq_config.heap_max_size, 512);
    assert_eq!(high_freq_config.precision_gap, Duration::from_micros(100));

    let low_freq_config = StalenessConfig::low_frequency();
    assert_eq!(low_freq_config.heap_max_size, 128);
    assert_eq!(low_freq_config.precision_gap, Duration::from_millis(10));

    let batch_config = StalenessConfig::batch_processing();
    assert_eq!(batch_config.heap_max_size, 64);
    assert_eq!(batch_config.timer_wheel_slots, 32);
}

#[tokio::test]
async fn test_staleness_with_lazy_checking() {
    // Test staleness with lazy checking (no real-time expiration)
    let messages = vec![
        ("A", TestMessage::new(1000, "a1")),
        ("B", TestMessage::new(1050, "b1")),
        ("A", TestMessage::new(2000, "a2")),
        ("B", TestMessage::new(2050, "b2")),
    ];

    let staleness_config = StalenessConfig {
        enable_immediate_expiration: false,            // Use lazy checking
        heap_time_horizon: Duration::from_millis(100), // Short horizon for testing
        ..StalenessConfig::default()
    };

    let config = Config::with_staleness(Duration::from_millis(100), None, 16, staleness_config);

    let stream = stream::iter(messages.into_iter().map(Ok));
    let (output_stream, _feedback_receiver) = sync(stream, vec!["A", "B"], config).unwrap();

    let groups: Vec<IndexMap<&str, TestMessage>> = output_stream.try_collect().await.unwrap();

    // With lazy checking and immediate message delivery, messages should synchronize
    assert!(!groups.is_empty());

    // Verify groups are valid
    for group in &groups {
        assert!(!group.is_empty());
    }
}

#[tokio::test]
async fn test_config_with_staleness() {
    // Test Config construction with staleness
    let staleness_config = StalenessConfig::default();
    let config = Config::with_staleness(
        Duration::from_millis(100),
        Some(Duration::from_millis(1000)),
        32,
        staleness_config.clone(),
    );

    assert_eq!(config.window_size, Duration::from_millis(100));
    assert_eq!(config.start_time, Some(Duration::from_millis(1000)));
    assert_eq!(config.buf_size, 32);
    assert!(config.staleness_config.is_some());

    // Test basic config without staleness
    let basic_config = Config::basic(Duration::from_millis(200), None, 16);

    assert_eq!(basic_config.window_size, Duration::from_millis(200));
    assert!(basic_config.staleness_config.is_none());

    // Test enabling staleness on existing config
    let enabled_config = basic_config.enable_staleness(staleness_config);
    assert!(enabled_config.staleness_config.is_some());
}

#[tokio::test]
async fn test_staleness_detector_stats() {
    // Test that staleness detector can be created and provides stats
    use conflux_core::staleness::StalenessDetector;

    let config = StalenessConfig::default();
    let mut detector: StalenessDetector<String, TestMessage> = StalenessDetector::new(config);

    // Initially, stats should show empty
    let stats = detector.stats();
    assert_eq!(stats.heap_size, 0);
    assert_eq!(stats.timer_wheel_size, 0);
    assert_eq!(stats.total_tracked, 0);

    // Add a message
    detector.add_message(
        "test".to_string(),
        TestMessage::new(1000, "test"),
        Duration::from_millis(100),
    );

    // Stats should show one tracked message
    let stats = detector.stats();
    assert!(stats.total_tracked > 0);
}
