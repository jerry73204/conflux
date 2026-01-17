mod common;

use common::*;
use conflux_core::WithTimestamp;
use std::time::Duration;

#[tokio::test]
async fn test_perfect_synchronization() {
    // Stream A: [1000ms, 2000ms, 3000ms]
    // Stream B: [1000ms, 2000ms, 3000ms]
    // Expected: 3 groups, each with messages from both streams

    let stream = StreamBuilder::new()
        .add_messages("A", &[1000, 2000, 3000])
        .add_messages("B", &[1000, 2000, 3000])
        .build();

    let config = config_with_window(50); // 50ms window
    let groups = run_sync(stream, ["A", "B"], config).await.unwrap();

    assert_eq!(groups.len(), 3);

    for group in &groups {
        assert_eq!(group.len(), 2);
        assert!(group.contains_key("A"));
        assert!(group.contains_key("B"));

        // Messages should have identical timestamps
        let ts_a = group["A"].timestamp();
        let ts_b = group["B"].timestamp();
        assert_eq!(ts_a, ts_b);
    }

    assert_groups_valid(&groups, Duration::from_millis(50));
    assert_timestamp_ordering(&groups);
}

#[tokio::test]
async fn test_close_synchronization() {
    // Stream A: [1000ms, 2000ms, 3000ms]
    // Stream B: [1010ms, 1990ms, 3020ms]
    // Window: 50ms
    // Expected: 3 groups

    let stream = StreamBuilder::new()
        .add_messages("A", &[1000, 2000, 3000])
        .add_messages("B", &[1010, 1990, 3020])
        .build();

    let config = config_with_window(50);
    let groups = run_sync(stream, ["A", "B"], config).await.unwrap();

    assert_eq!(groups.len(), 3);

    for group in &groups {
        assert_eq!(group.len(), 2);
        assert!(group.contains_key("A"));
        assert!(group.contains_key("B"));
    }

    // Verify specific groupings
    assert_eq!(groups[0]["A"].timestamp(), Duration::from_millis(1000));
    assert_eq!(groups[0]["B"].timestamp(), Duration::from_millis(1010));

    assert_eq!(groups[1]["A"].timestamp(), Duration::from_millis(2000));
    assert_eq!(groups[1]["B"].timestamp(), Duration::from_millis(1990));

    assert_eq!(groups[2]["A"].timestamp(), Duration::from_millis(3000));
    assert_eq!(groups[2]["B"].timestamp(), Duration::from_millis(3020));

    assert_groups_valid(&groups, Duration::from_millis(50));
    assert_timestamp_ordering(&groups);
}

#[tokio::test]
async fn test_failed_synchronization() {
    // Stream A: [1000ms, 2000ms]
    // Stream B: [1200ms, 2200ms]
    // Window: 50ms
    // Expected: 0 groups (messages too far apart)

    let stream = StreamBuilder::new()
        .add_messages("A", &[1000, 2000])
        .add_messages("B", &[1200, 2200])
        .build();

    let config = config_with_window(50);
    let groups = run_sync(stream, ["A", "B"], config).await.unwrap();

    assert_eq!(groups.len(), 0);
}

#[tokio::test]
async fn test_sequential_groups() {
    // Stream A: [1000ms, 1500ms, 2000ms, 2500ms]
    // Stream B: [1020ms, 1480ms, 2010ms, 2520ms]
    // Window: 100ms
    // Expected: 4 groups

    let stream = StreamBuilder::new()
        .add_messages("A", &[1000, 1500, 2000, 2500])
        .add_messages("B", &[1020, 1480, 2010, 2520])
        .build();

    let config = config_with_window(100);
    let groups = run_sync(stream, ["A", "B"], config).await.unwrap();

    assert_eq!(groups.len(), 4);

    for group in &groups {
        assert_eq!(group.len(), 2);
        assert!(group.contains_key("A"));
        assert!(group.contains_key("B"));
    }

    assert_groups_valid(&groups, Duration::from_millis(100));
    assert_timestamp_ordering(&groups);
}

#[tokio::test]
async fn test_uneven_message_counts() {
    // Stream A: [1000ms, 2000ms, 3000ms, 4000ms, 5000ms]
    // Stream B: [1010ms, 3010ms]
    // Window: 50ms
    // Expected: Limited groups due to sparse stream B

    let stream = StreamBuilder::new()
        .add_messages("A", &[1000, 2000, 3000, 4000, 5000])
        .add_messages("B", &[1010, 3010])
        .build();

    let config = config_with_window(50);
    let groups = run_sync(stream, ["A", "B"], config).await.unwrap();

    // Should have at least 1 group, possibly more depending on algorithm behavior
    assert!(!groups.is_empty());
    assert!(groups.len() <= 2); // Limited by the sparse stream B

    // All formed groups should have both streams
    for group in &groups {
        assert_eq!(group.len(), 2);
        assert!(group.contains_key("A"));
        assert!(group.contains_key("B"));
    }

    assert_groups_valid(&groups, Duration::from_millis(50));
    assert_timestamp_ordering(&groups);
}

#[tokio::test]
async fn test_large_window_groups_many() {
    // Stream A: [1000ms, 2000ms, 3000ms]
    // Stream B: [1500ms, 2500ms, 3500ms]
    // Window: 1000ms (large window)
    // Expected: Groups should form within the large window

    let stream = StreamBuilder::new()
        .add_messages("A", &[1000, 2000, 3000])
        .add_messages("B", &[1500, 2500, 3500])
        .build();

    let config = config_with_window(1000);
    let groups = run_sync(stream, ["A", "B"], config).await.unwrap();

    // Should form groups, but exact count depends on algorithm behavior
    assert!(!groups.is_empty());

    for group in &groups {
        assert_eq!(group.len(), 2);
    }

    assert_groups_valid(&groups, Duration::from_millis(1000));
    assert_timestamp_ordering(&groups);
}

#[tokio::test]
async fn test_single_message_each_stream() {
    // Stream A: [1000ms]
    // Stream B: [1020ms]
    // Window: 50ms
    // Expected: 1 group

    let stream = StreamBuilder::new()
        .add_message("A", 1000)
        .add_message("B", 1020)
        .build();

    let config = config_with_window(50);
    let groups = run_sync(stream, ["A", "B"], config).await.unwrap();

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].len(), 2);
    assert_eq!(groups[0]["A"].timestamp(), Duration::from_millis(1000));
    assert_eq!(groups[0]["B"].timestamp(), Duration::from_millis(1020));

    assert_groups_valid(&groups, Duration::from_millis(50));
}

#[tokio::test]
async fn test_window_boundary_exact() {
    // Stream A: [1000ms, 2000ms]
    // Stream B: [1100ms, 2100ms] (exactly 100ms apart)
    // Window: 100ms
    // Expected: 2 groups (messages exactly at window boundary)

    let stream = StreamBuilder::new()
        .add_messages("A", &[1000, 2000])
        .add_messages("B", &[1100, 2100])
        .build();

    let config = config_with_window(100);
    let groups = run_sync(stream, ["A", "B"], config).await.unwrap();

    assert_eq!(groups.len(), 2);

    for group in &groups {
        assert_eq!(group.len(), 2);
    }

    assert_groups_valid(&groups, Duration::from_millis(100));
    assert_timestamp_ordering(&groups);
}
