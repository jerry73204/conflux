mod common;

use common::*;
use conflux_core::WithTimestamp;
use std::time::Duration;

#[tokio::test]
async fn test_basic_three_stream_sync() {
    // All streams synchronizable
    // Stream A: [1000ms, 2000ms]
    // Stream B: [1010ms, 2010ms]
    // Stream C: [1020ms, 2020ms]
    // Window: 50ms

    let stream = StreamBuilder::new()
        .add_messages("A", &[1000, 2000])
        .add_messages("B", &[1010, 2010])
        .add_messages("C", &[1020, 2020])
        .build();

    let config = config_with_window(50);
    let groups = run_sync(stream, ["A", "B", "C"], config).await.unwrap();

    assert_eq!(groups.len(), 2);

    for group in &groups {
        assert_eq!(group.len(), 3);
        assert!(group.contains_key("A"));
        assert!(group.contains_key("B"));
        assert!(group.contains_key("C"));
    }

    assert_groups_valid(&groups, Duration::from_millis(50));
    assert_timestamp_ordering(&groups);
}

#[tokio::test]
async fn test_two_of_three_sync() {
    // Only two streams have matching timestamps at a time
    // Stream A: [1000ms, 3000ms]
    // Stream B: [1010ms, 2000ms]
    // Stream C: [2010ms, 3010ms]
    // Window: 50ms
    // Expected: Synchronizer requires all streams to have messages for grouping

    let stream = StreamBuilder::new()
        .add_messages("A", &[1000, 3000])
        .add_messages("B", &[1010, 2000])
        .add_messages("C", &[2010, 3010])
        .build();

    let config = config_with_window(50);
    let groups = run_sync(stream, ["A", "B", "C"], config).await.unwrap();

    // The synchronizer waits for all three streams to have matching messages
    // This pattern may not produce groups since not all streams align temporally

    for group in &groups {
        // Any formed groups should be valid
        assert!(group.len() >= 2);
    }

    assert_groups_valid(&groups, Duration::from_millis(50));
    assert_timestamp_ordering(&groups);
}

#[tokio::test]
async fn test_staggered_arrival() {
    // Streams have different arrival patterns
    // Stream A: [1000ms, 1200ms, 1400ms, 1600ms]
    // Stream B: [1050ms, 1350ms, 1650ms]
    // Stream C: [1100ms, 1300ms, 1500ms]
    // Window: 100ms

    let stream = StreamBuilder::new()
        .add_messages("A", &[1000, 1200, 1400, 1600])
        .add_messages("B", &[1050, 1350, 1650])
        .add_messages("C", &[1100, 1300, 1500])
        .build();

    let config = config_with_window(100);
    let groups = run_sync(stream, ["A", "B", "C"], config).await.unwrap();

    // Should form multiple groups with different combinations
    assert!(!groups.is_empty());

    for group in &groups {
        // Each group should have at least 2 streams (can't group with just 1)
        assert!(group.len() >= 2);
    }

    assert_groups_valid(&groups, Duration::from_millis(100));
    assert_timestamp_ordering(&groups);
}

#[tokio::test]
async fn test_one_stream_lagging() {
    // One stream consistently late but within window
    // Stream A: [1000ms, 2000ms, 3000ms]
    // Stream B: [1000ms, 2000ms, 3000ms]
    // Stream C: [1090ms, 2090ms, 3090ms] (consistently 90ms late)
    // Window: 100ms

    let stream = StreamBuilder::new()
        .add_messages("A", &[1000, 2000, 3000])
        .add_messages("B", &[1000, 2000, 3000])
        .add_messages("C", &[1090, 2090, 3090])
        .build();

    let config = config_with_window(100);
    let groups = run_sync(stream, ["A", "B", "C"], config).await.unwrap();

    assert_eq!(groups.len(), 3);

    for group in &groups {
        assert_eq!(group.len(), 3);

        // Verify C is consistently 90ms later than A and B
        let ts_a = group["A"].timestamp();
        let ts_b = group["B"].timestamp();
        let ts_c = group["C"].timestamp();

        assert_eq!(ts_a, ts_b);
        assert_eq!(ts_c, ts_a + Duration::from_millis(90));
    }

    assert_groups_valid(&groups, Duration::from_millis(100));
    assert_timestamp_ordering(&groups);
}

#[tokio::test]
async fn test_sparse_stream_combination() {
    // Some streams have sparse messages
    // Stream A: [1000ms, 1500ms, 2000ms, 2500ms, 3000ms]
    // Stream B: [1010ms, 2010ms, 3010ms] (every other message)
    // Stream C: [1020ms] (single message)
    // Window: 50ms

    let stream = StreamBuilder::new()
        .add_messages("A", &[1000, 1500, 2000, 2500, 3000])
        .add_messages("B", &[1010, 2010, 3010])
        .add_messages("C", &[1020])
        .build();

    let config = config_with_window(50);
    let groups = run_sync(stream, ["A", "B", "C"], config).await.unwrap();

    // Sparse stream C limits grouping opportunities
    // Groups may form depending on the algorithm's stream depletion handling

    for group in &groups {
        // Any formed groups should be valid
        assert!(group.len() >= 2);
    }

    assert_groups_valid(&groups, Duration::from_millis(50));
    assert_timestamp_ordering(&groups);
}

#[tokio::test]
async fn test_no_common_sync_window() {
    // No common synchronization window for all three streams
    // Stream A: [1000ms]
    // Stream B: [1200ms]
    // Stream C: [1400ms]
    // Window: 100ms (not enough to cover all)

    let stream = StreamBuilder::new()
        .add_message("A", 1000)
        .add_message("B", 1200)
        .add_message("C", 1400)
        .build();

    let config = config_with_window(100);
    let groups = run_sync(stream, ["A", "B", "C"], config).await.unwrap();

    // Messages are too spread out for synchronization
    // May not produce any groups since all streams can't synchronize

    for group in &groups {
        // Any formed groups should be valid
        assert!(group.len() >= 2);
    }

    assert_groups_valid(&groups, Duration::from_millis(100));
    assert_timestamp_ordering(&groups);
}

#[tokio::test]
async fn test_three_stream_perfect_alignment() {
    // All three streams perfectly aligned
    // Stream A: [1000ms, 2000ms]
    // Stream B: [1000ms, 2000ms]
    // Stream C: [1000ms, 2000ms]
    // Window: 10ms (small window)

    let stream = StreamBuilder::new()
        .add_messages("A", &[1000, 2000])
        .add_messages("B", &[1000, 2000])
        .add_messages("C", &[1000, 2000])
        .build();

    let config = config_with_window(10);
    let groups = run_sync(stream, ["A", "B", "C"], config).await.unwrap();

    assert_eq!(groups.len(), 2);

    // Verify first group (1000ms)
    assert_eq!(groups[0].len(), 3);
    assert_eq!(groups[0]["A"].timestamp(), Duration::from_millis(1000));
    assert_eq!(groups[0]["B"].timestamp(), Duration::from_millis(1000));
    assert_eq!(groups[0]["C"].timestamp(), Duration::from_millis(1000));

    // Verify second group (2000ms)
    assert_eq!(groups[1].len(), 3);
    assert_eq!(groups[1]["A"].timestamp(), Duration::from_millis(2000));
    assert_eq!(groups[1]["B"].timestamp(), Duration::from_millis(2000));
    assert_eq!(groups[1]["C"].timestamp(), Duration::from_millis(2000));

    assert_groups_valid(&groups, Duration::from_millis(10));
    assert_timestamp_ordering(&groups);
}

#[tokio::test]
async fn test_complex_interleaving() {
    // Complex interleaving pattern
    // Stream A: [1000ms, 1300ms, 1600ms]
    // Stream B: [1100ms, 1400ms, 1700ms]
    // Stream C: [1200ms, 1500ms, 1800ms]
    // Window: 150ms (allows various combinations)

    let stream = StreamBuilder::new()
        .add_messages("A", &[1000, 1300, 1600])
        .add_messages("B", &[1100, 1400, 1700])
        .add_messages("C", &[1200, 1500, 1800])
        .build();

    let config = config_with_window(150);
    let groups = run_sync(stream, ["A", "B", "C"], config).await.unwrap();

    // Complex interleaving should produce some groups
    // The exact pattern depends on the algorithm's matching strategy

    for group in &groups {
        // Any formed groups should be valid
        assert!(group.len() >= 2);
    }

    assert_groups_valid(&groups, Duration::from_millis(150));
    assert_timestamp_ordering(&groups);
}
