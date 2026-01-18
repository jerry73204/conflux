mod common;

use common::*;
use conflux_core::{Config, WithTimestamp, sync};
use futures::{TryStreamExt, stream};
use std::time::Duration;

#[tokio::test]
async fn test_timestamp_boundary_cases() {
    // Test messages at exact window boundaries
    let window_size = 100u64;

    // Messages exactly at window boundaries
    let stream = StreamBuilder::new()
        .add_message("A", 1000)
        .add_message("B", 1000 + window_size) // Exactly at boundary
        .add_message("A", 2000)
        .add_message("B", 2000 + window_size - 1) // Just within boundary
        .add_message("A", 3000)
        .add_message("B", 3000 + window_size + 1) // Just outside boundary
        .build();

    let config = config_with_window(window_size);
    let groups = run_sync(stream, ["A", "B"], config).await.unwrap();

    // Boundary behavior verification
    // Exact boundary cases depend on algorithm implementation
    assert_groups_valid(&groups, Duration::from_millis(window_size));
    assert_timestamp_ordering(&groups);
    assert_eq!(groups.len(), 2);

    println!("Boundary test: {} groups formed", groups.len());

    // Log which boundaries worked
    for (i, group) in groups.iter().enumerate() {
        let diff =
            group["B"].timestamp().as_millis() as i64 - group["A"].timestamp().as_millis() as i64;
        println!("Group {}: time difference = {}ms", i, diff);
    }
}

#[tokio::test]
async fn test_zero_timestamp_handling() {
    // Test with timestamp zero and very small timestamps
    let stream = StreamBuilder::new()
        .add_messages("A", &[0, 1, 10, 100])
        .add_messages("B", &[0, 2, 15, 105])
        .build();

    let config = config_with_window(50);
    let groups = run_sync(stream, ["A", "B"], config).await.unwrap();

    assert!(!groups.is_empty());
    assert_eq!(groups.len(), 4);

    // Verify zero timestamp is handled correctly
    assert_groups_valid(&groups, Duration::from_millis(50));
    assert_timestamp_ordering(&groups);

    println!(
        "Zero timestamp: {} groups, first group A={}ms B={}ms",
        groups.len(),
        groups[0]["A"].timestamp().as_millis(),
        groups[0]["B"].timestamp().as_millis()
    );
}

#[tokio::test]
async fn test_single_message_streams() {
    // Edge case: streams with only one message each
    let stream = StreamBuilder::new()
        .add_message("solo_1", 1000)
        .add_message("solo_2", 1020)
        .add_message("solo_3", 1040)
        .build();

    let config = config_with_window(100);
    let groups = run_sync(stream, ["solo_1", "solo_2", "solo_3"], config)
        .await
        .unwrap();

    // Should form one group with all three messages
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].len(), 3);

    assert_groups_valid(&groups, Duration::from_millis(100));

    println!(
        "Single message streams: formed one group with {} streams",
        groups[0].len()
    );
}

#[tokio::test]
async fn test_identical_timestamps() {
    // Multiple messages with exactly identical timestamps
    let timestamp = 5000u64;

    let stream = StreamBuilder::new()
        .add_message("A", timestamp)
        .add_message("B", timestamp)
        .add_message("C", timestamp)
        .add_message("A", timestamp + 1000)
        .add_message("B", timestamp + 1000)
        .add_message("C", timestamp + 1000)
        .build();

    let config = config_with_window(10); // Very small window
    let groups = run_sync(stream, ["A", "B", "C"], config).await.unwrap();

    assert_eq!(groups.len(), 2);

    // Both groups should have identical timestamps within each group
    for group in &groups {
        assert_eq!(group.len(), 3);
        let timestamps: Vec<Duration> = group.values().map(|msg| msg.timestamp()).collect();
        let first_ts = timestamps[0];
        assert!(timestamps.iter().all(|&ts| ts == first_ts));
    }

    assert_groups_valid(&groups, Duration::from_millis(10));
    assert_timestamp_ordering(&groups);

    println!(
        "Identical timestamps: {} groups with perfect alignment",
        groups.len()
    );
}

#[tokio::test]
async fn test_feedback_mechanism_validation() {
    // Test the full feedback mechanism (if available)
    let stream = StreamBuilder::new()
        .add_messages("A", &[100, 200, 300, 400, 500])
        .add_messages("B", &[120, 220, 320, 420, 520])
        .build();

    let config = config_with_window(50);
    let (output_stream, feedback_receiver) = sync(stream, ["A", "B"], config).unwrap();

    // Collect both outputs and feedback
    let groups: Vec<_> = output_stream.try_collect().await.unwrap();

    // Check if feedback channel provides useful information
    let feedback = feedback_receiver.borrow();
    println!("Current feedback: {:?}", *feedback);

    assert!(!groups.is_empty());
    assert_groups_valid(&groups, Duration::from_millis(50));

    println!("Feedback test: {} groups formed", groups.len());
}

#[tokio::test]
async fn test_window_size_edge_cases() {
    // Test with minimum and maximum practical window sizes
    let timestamps_a = [1000, 2000, 3000];
    let timestamps_b = [1001, 2001, 3001];

    // Test with window size = 1ms (minimal)
    let stream = StreamBuilder::new()
        .add_messages("A", &timestamps_a)
        .add_messages("B", &timestamps_b)
        .build();

    let config = config_with_window(1);
    let groups = run_sync(stream, ["A", "B"], config).await.unwrap();

    assert_eq!(groups.len(), 3); // 1ms difference should sync with 1ms window
    assert_groups_valid(&groups, Duration::from_millis(1));

    // Test with very large window (10 seconds)
    let stream2 = StreamBuilder::new()
        .add_messages("A", &[1000, 5000, 9000])
        .add_messages("B", &[2000, 6000, 8000])
        .build();

    let config2 = config_with_window(10_000);
    let groups2 = run_sync(stream2, ["A", "B"], config2).await.unwrap();

    assert!(!groups2.is_empty());
    assert_groups_valid(&groups2, Duration::from_millis(10_000));

    println!(
        "Window edge cases: 1ms window -> {} groups, 10s window -> {} groups",
        groups.len(),
        groups2.len()
    );
}

#[tokio::test]
async fn test_buffer_size_edge_cases() {
    // Test with minimum buffer size (2) and very large sizes
    let large_message_set: Vec<u64> = (0..1000).map(|i| i * 10).collect();

    // Minimum buffer size = 2
    let stream1 = StreamBuilder::new()
        .add_messages("A", &large_message_set[..100])
        .add_messages(
            "B",
            &large_message_set[..100]
                .iter()
                .map(|&t| t + 5)
                .collect::<Vec<_>>(),
        )
        .build();

    let config1 = Config::basic(Some(Duration::from_millis(50)), None, 2);

    let groups1 = run_sync(stream1, ["A", "B"], config1).await.unwrap();

    assert!(!groups1.is_empty());
    assert_groups_valid(&groups1, Duration::from_millis(50));

    // Very large buffer size
    let stream2 = StreamBuilder::new()
        .add_messages("A", &large_message_set)
        .add_messages(
            "B",
            &large_message_set.iter().map(|&t| t + 5).collect::<Vec<_>>(),
        )
        .build();

    let config2 = Config::basic(Some(Duration::from_millis(50)), None, 10_000);

    let groups2 = run_sync(stream2, ["A", "B"], config2).await.unwrap();

    assert!(!groups2.is_empty());
    assert_groups_valid(&groups2, Duration::from_millis(50));

    println!(
        "Buffer edge cases: size=2 -> {} groups, size=10k -> {} groups",
        groups1.len(),
        groups2.len()
    );
}

#[tokio::test]
async fn test_start_time_configurations() {
    // Test different start_time configurations
    let all_timestamps = [500, 1000, 1500, 2000, 2500, 3000];

    // No start time (default)
    let stream1 = StreamBuilder::new()
        .add_messages("A", &all_timestamps)
        .add_messages(
            "B",
            &all_timestamps.iter().map(|&t| t + 10).collect::<Vec<_>>(),
        )
        .build();

    let config1 = Config::basic(Some(Duration::from_millis(50)), None, 16);

    let groups1 = run_sync(stream1, ["A", "B"], config1).await.unwrap();

    // Start time at 1500ms (should skip early messages)
    let stream2 = StreamBuilder::new()
        .add_messages("A", &all_timestamps)
        .add_messages(
            "B",
            &all_timestamps.iter().map(|&t| t + 10).collect::<Vec<_>>(),
        )
        .build();

    let config2 = Config::basic(
        Some(Duration::from_millis(50)),
        Some(Duration::from_millis(1500)),
        16,
    );

    let groups2 = run_sync(stream2, ["A", "B"], config2).await.unwrap();

    // Should have fewer groups with start_time filter
    assert!(groups2.len() < groups1.len());

    // All messages in groups2 should be after start_time
    for group in &groups2 {
        for msg in group.values() {
            assert!(msg.timestamp() >= Duration::from_millis(1500));
        }
    }

    assert_groups_valid(&groups1, Duration::from_millis(50));
    assert_groups_valid(&groups2, Duration::from_millis(50));

    println!(
        "Start time test: no filter -> {} groups, filter@1500ms -> {} groups",
        groups1.len(),
        groups2.len()
    );
}

#[tokio::test]
async fn test_stream_order_independence() {
    // Verify that message arrival order doesn't affect final grouping
    let _timestamps_a = [1000, 2000, 3000]; // Reserved for future use
    let _timestamps_b = [1010, 2010, 3010]; // Reserved for future use

    // Ordered arrival
    let ordered_stream = stream::iter([
        Ok(("A", create_message(1000))),
        Ok(("B", create_message(1010))),
        Ok(("A", create_message(2000))),
        Ok(("B", create_message(2010))),
        Ok(("A", create_message(3000))),
        Ok(("B", create_message(3010))),
    ]);

    // Interleaved arrival
    let interleaved_stream = stream::iter([
        Ok(("A", create_message(1000))),
        Ok(("A", create_message(2000))),
        Ok(("B", create_message(1010))),
        Ok(("A", create_message(3000))),
        Ok(("B", create_message(2010))),
        Ok(("B", create_message(3010))),
    ]);

    let config = config_with_window(50);

    let groups1 = run_sync(ordered_stream, ["A", "B"], config.clone())
        .await
        .unwrap();
    let groups2 = run_sync(interleaved_stream, ["A", "B"], config)
        .await
        .unwrap();

    // Results should be identical regardless of arrival order
    assert_eq!(groups1.len(), groups2.len());

    assert_groups_valid(&groups1, Duration::from_millis(50));
    assert_groups_valid(&groups2, Duration::from_millis(50));
    assert_timestamp_ordering(&groups1);
    assert_timestamp_ordering(&groups2);

    println!(
        "Order independence: both orderings produced {} groups",
        groups1.len()
    );
}

#[tokio::test]
async fn test_concurrent_identical_streams() {
    // Edge case: multiple streams with identical timing
    let timestamps = [1000, 2000, 3000, 4000];

    let stream = StreamBuilder::new()
        .add_messages("stream_1", &timestamps)
        .add_messages("stream_2", &timestamps) // Identical
        .add_messages("stream_3", &timestamps) // Identical
        .add_messages("stream_4", &timestamps) // Identical
        .build();

    let config = config_with_window(10); // Small window for perfect sync only
    let groups = run_sync(
        stream,
        ["stream_1", "stream_2", "stream_3", "stream_4"],
        config,
    )
    .await
    .unwrap();

    // Should form groups with all four streams
    assert_eq!(groups.len(), timestamps.len());

    for group in &groups {
        assert_eq!(group.len(), 4);

        // All timestamps should be identical within each group
        let timestamps: Vec<Duration> = group.values().map(|msg| msg.timestamp()).collect();
        let first_ts = timestamps[0];
        assert!(timestamps.iter().all(|&ts| ts == first_ts));
    }

    assert_groups_valid(&groups, Duration::from_millis(10));
    assert_timestamp_ordering(&groups);

    println!(
        "Concurrent identical: {} groups with {} streams each",
        groups.len(),
        4
    );
}
