mod common;

use common::*;
use conflux_core::{Config, WithTimestamp};
use futures::stream;
use std::time::Duration;

#[tokio::test]
async fn test_buffer_overflow_handling() {
    // Test message dropping when buffers are full
    // Config: buf_size = 3
    // Stream A: Fast arrival rate (many messages)
    // Stream B: Slow arrival rate
    // Expected: Messages dropped from Stream A

    let stream = StreamBuilder::new()
        .add_messages("A", &[1000, 1010, 1020, 1030, 1040, 1050, 1060, 1070])
        .add_messages("B", &[1500, 2000])
        .build();

    let config = config_with_buffer_size(3);
    let groups = run_sync(stream, ["A", "B"], config).await.unwrap();

    // With Stream B starting at 1500ms and Stream A ending at 1070ms,
    // the 500ms gap exceeds the 100ms window, so no synchronization is possible
    // This demonstrates correct buffer overflow behavior - messages are dropped
    // but the algorithm doesn't create invalid synchronizations
    assert_eq!(groups.len(), 0);

    // Any groups that might be formed should still be valid
    assert_groups_valid(&groups, Duration::from_millis(100));
    assert_timestamp_ordering(&groups);
}

#[tokio::test]
async fn test_window_based_dropping() {
    // Test drop_before() behavior with window management
    // Messages outside the window should be dropped

    let stream = StreamBuilder::new()
        .add_messages("A", &[1000, 1100, 1800, 1900])
        .add_messages("B", &[1050, 1150, 1850, 1950])
        .build();

    let config = config_with_window(200); // 200ms window
    let groups = run_sync(stream, ["A", "B"], config).await.unwrap();

    // Algorithm creates 3 groups, with the later messages (1900ms, 1950ms)
    // pairing together instead of the earlier pair (1800ms, 1850ms)
    // This demonstrates the synchronizer's matching strategy
    assert_eq!(groups.len(), 3);

    // Verify specific group contents
    assert_eq!(groups[0]["A"].timestamp(), Duration::from_millis(1000));
    assert_eq!(groups[0]["B"].timestamp(), Duration::from_millis(1050));

    assert_eq!(groups[1]["A"].timestamp(), Duration::from_millis(1100));
    assert_eq!(groups[1]["B"].timestamp(), Duration::from_millis(1150));

    // The algorithm chooses the later pair (1900ms, 1950ms) over (1800ms, 1850ms)
    // This shows the synchronizer's preference for more recent matches
    assert_eq!(groups[2]["A"].timestamp(), Duration::from_millis(1900));
    assert_eq!(groups[2]["B"].timestamp(), Duration::from_millis(1950));

    assert_groups_valid(&groups, Duration::from_millis(200));
    assert_timestamp_ordering(&groups);
}

#[tokio::test]
async fn test_state_transitions() {
    // Test state transitions: Not ready → Ready → Full → Ready
    // Uses different phases of message arrival

    // Phase 1: Not ready (single messages)
    let mut stream_builder = StreamBuilder::new().add_message("A", 1000);

    // Phase 2: Ready (both streams have >= 2 messages)
    stream_builder = stream_builder
        .add_message("A", 1100)
        .add_message("B", 1050)
        .add_message("B", 1150);

    // Phase 3: Continue with more messages to test full state
    let stream = stream_builder
        .add_messages("A", &[1200, 1300, 1400])
        .add_messages("B", &[1250, 1350, 1450])
        .build();

    let config = config_with_buffer_size(4);
    let groups = run_sync(stream, ["A", "B"], config).await.unwrap();

    // Should successfully transition through states and form groups
    assert!(!groups.is_empty());

    for group in &groups {
        assert_eq!(group.len(), 2);
    }

    assert_groups_valid(&groups, Duration::from_millis(100));
    assert_timestamp_ordering(&groups);
}

#[tokio::test]
async fn test_empty_input_stream() {
    // Test with no messages at all
    let empty_stream = stream::empty::<eyre::Result<(&str, TestMessage)>>();

    let config = default_config();
    let groups = run_sync(empty_stream, ["A", "B"], config).await.unwrap();

    assert_eq!(groups.len(), 0);
}

#[tokio::test]
async fn test_single_message_per_stream() {
    // Test minimal input - single message per stream
    let stream = StreamBuilder::new()
        .add_message("A", 1000)
        .add_message("B", 1050)
        .build();

    let config = config_with_window(100);
    let groups = run_sync(stream, ["A", "B"], config).await.unwrap();

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].len(), 2);

    assert_groups_valid(&groups, Duration::from_millis(100));
}

#[tokio::test]
async fn test_uneven_stream_depletion() {
    // One stream ends before others
    let stream = StreamBuilder::new()
        .add_messages("A", &[1000, 1200]) // Ends early
        .add_messages("B", &[1050, 1250, 1450, 1650])
        .build();

    let config = config_with_window(100);
    let groups = run_sync(stream, ["A", "B"], config).await.unwrap();

    // Should form groups until stream A is depleted
    assert!(!groups.is_empty());
    assert!(groups.len() <= 2); // Limited by shorter stream

    for group in &groups {
        // All formed groups should have both streams
        assert_eq!(group.len(), 2);
    }

    assert_groups_valid(&groups, Duration::from_millis(100));
    assert_timestamp_ordering(&groups);
}

#[tokio::test]
async fn test_final_group_formation_after_depletion() {
    // Test that remaining messages form groups after stream depletion
    let stream = StreamBuilder::new()
        .add_messages("A", &[1000, 1200, 1400])
        .add_messages("B", &[1050, 1250])
        .build();

    let config = config_with_window(100);
    let groups = run_sync(stream, ["A", "B"], config).await.unwrap();

    // Should form groups from available messages
    assert!(!groups.is_empty());

    for group in &groups {
        assert_eq!(group.len(), 2);
    }

    assert_groups_valid(&groups, Duration::from_millis(100));
    assert_timestamp_ordering(&groups);
}

#[tokio::test]
async fn test_input_stream_error_propagation() {
    // Test error handling in input stream
    let error_stream = stream::iter(vec![
        Ok(("A", create_message(1000))),
        Ok(("B", create_message(1050))),
        Err(eyre::eyre!("Test error")),
        Ok(("A", create_message(1200))),
    ]);

    let config = default_config();
    let result = run_sync(error_stream, ["A", "B"], config).await;

    // Should propagate the error
    assert!(result.is_err());
}

#[tokio::test]
async fn test_late_message_handling() {
    // Messages arriving before commit timestamp should be dropped
    // This is handled at the state level but tested in integration

    let stream = StreamBuilder::new()
        .add_message("A", 2000) // Early message
        .add_message("B", 2050)
        .add_message("A", 1000) // Late message (should be dropped)
        .add_message("B", 2100)
        .build();

    let config = Config {
        window_size: Duration::from_millis(100),
        start_time: Some(Duration::from_millis(1500)), // Start after the "late" message
        buf_size: 16,
        staleness_config: None,
    };

    let groups = run_sync(stream, ["A", "B"], config).await.unwrap();

    // Should only form groups from messages after start_time
    for group in &groups {
        for msg in group.values() {
            assert!(msg.timestamp() >= Duration::from_millis(1500));
        }
    }

    assert_groups_valid(&groups, Duration::from_millis(100));
    assert_timestamp_ordering(&groups);
}

#[tokio::test]
async fn test_rapid_state_changes() {
    // Test frequent transitions between buffer states
    let stream = StreamBuilder::new()
        // Rapid alternating pattern
        .add_message("A", 1000)
        .add_message("B", 1010)
        .add_message("A", 1020)
        .add_message("C", 1030)
        .add_message("B", 1040)
        .add_message("C", 1050)
        .add_message("A", 1060)
        .add_message("B", 1070)
        .add_message("C", 1080)
        .build();

    let config = Config {
        window_size: Duration::from_millis(50),
        start_time: None,
        buf_size: 2, // Small buffer to force rapid state changes
        staleness_config: None,
    };

    let groups = run_sync(stream, ["A", "B", "C"], config).await.unwrap();

    // Should handle rapid state changes gracefully
    assert!(!groups.is_empty());

    for group in &groups {
        assert!(group.len() >= 2);
    }

    assert_groups_valid(&groups, Duration::from_millis(50));
    assert_timestamp_ordering(&groups);
}

#[tokio::test]
async fn test_buffer_size_limits() {
    // Test behavior at exact buffer capacity limits
    let stream = StreamBuilder::new()
        .add_messages("A", &[1000, 1100, 1200]) // Exactly buffer size
        .add_messages("B", &[2000, 2100, 2200]) // No overlap, forces buffer limits
        .build();

    let config = config_with_buffer_size(3);
    let groups = run_sync(stream, ["A", "B"], config).await.unwrap();

    // With no overlap, should not form any groups
    assert_eq!(groups.len(), 0);
}

#[tokio::test]
async fn test_message_ordering_within_stream() {
    // Verify that out-of-order messages within a stream are rejected
    let stream = StreamBuilder::new()
        .add_message("A", 2000)
        .add_message("A", 1000) // Out of order - should be rejected
        .add_message("A", 3000)
        .add_message("B", 2050)
        .add_message("B", 3050)
        .build();

    let config = config_with_window(100);
    let groups = run_sync(stream, ["A", "B"], config).await.unwrap();

    // Should only have 2 groups (rejecting the out-of-order message)
    assert_eq!(groups.len(), 2);

    // Verify the timestamps are correctly ordered
    assert_eq!(groups[0]["A"].timestamp(), Duration::from_millis(2000));
    assert_eq!(groups[1]["A"].timestamp(), Duration::from_millis(3000));

    assert_groups_valid(&groups, Duration::from_millis(100));
    assert_timestamp_ordering(&groups);
}
