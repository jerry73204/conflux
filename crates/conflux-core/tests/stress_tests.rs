mod common;

use common::*;
use conflux_core::{Config, WithTimestamp};
use std::time::{Duration, Instant};

#[tokio::test]
async fn test_high_volume_processing() {
    // Process 10k+ messages efficiently
    let message_count = 10_000usize;
    let stream_count = 3;
    let interval = 10; // 10ms intervals

    // Create interleaved messages to simulate realistic sensor data
    // Messages from different streams arrive interleaved by timestamp
    let mut builder = StreamBuilder::new();
    let keys: Vec<String> = (0..stream_count).map(|i| format!("stream_{}", i)).collect();

    for i in 0..message_count {
        for (stream_id, key) in keys.iter().enumerate() {
            let timestamp = (i * interval + stream_id * 2) as u64;
            builder = builder.add_message(key.as_str(), timestamp);
        }
    }

    let key_refs: Vec<&str> = keys.iter().map(|s| s.as_str()).collect();
    let stream = builder.build();

    // Use reasonable buffer for high-volume processing
    let config = Config::basic(Some(Duration::from_millis(50)), None, 64);

    let start_time = Instant::now();
    let groups = run_sync(stream, key_refs, config).await.unwrap();
    let elapsed = start_time.elapsed();

    // Performance requirements
    assert!(
        elapsed.as_millis() < 5000,
        "Processing took too long: {:?}",
        elapsed
    );

    // Should process most messages efficiently
    assert!(!groups.is_empty());
    assert!(
        groups.len() > message_count / 2,
        "Too few groups formed: {}",
        groups.len()
    );

    assert_groups_valid(&groups, Duration::from_millis(50));
    assert_timestamp_ordering(&groups);

    println!(
        "High volume: {} groups from {}k messages in {:?}",
        groups.len(),
        message_count / 1000,
        elapsed
    );
}

#[tokio::test]
async fn test_many_concurrent_streams() {
    // Test with 10+ concurrent streams
    let stream_count = 12;
    let messages_per_stream = 100;
    let base_interval = 100; // 100ms base interval

    let mut builder = StreamBuilder::new();

    // Use static keys for consistency
    let keys = [
        "s00", "s01", "s02", "s03", "s04", "s05", "s06", "s07", "s08", "s09", "s10", "s11",
    ];
    let key_slice = &keys[..stream_count];

    for (stream_id, &key) in key_slice.iter().enumerate() {
        // Each stream has slightly different timing to test synchronization
        let timestamps: Vec<u64> = (0..messages_per_stream)
            .map(|i| i * base_interval + (stream_id * 5) as u64) // 5ms offset per stream
            .collect();

        builder = builder.add_messages(key, &timestamps);
    }
    let stream = builder.build();

    let config = config_with_window(100); // Generous window for many streams

    let start_time = Instant::now();
    let groups = run_sync(stream, key_slice.to_vec(), config).await.unwrap();
    let elapsed = start_time.elapsed();

    // Should handle many streams effectively
    assert!(!groups.is_empty());

    // Each group should contain messages from all streams
    for group in &groups {
        assert!(
            group.len() >= stream_count / 2,
            "Group has too few streams: {}",
            group.len()
        );
    }

    assert_groups_valid(&groups, Duration::from_millis(100));
    assert_timestamp_ordering(&groups);

    println!(
        "Many streams: {} groups from {} streams with {} msg/stream in {:?}",
        groups.len(),
        stream_count,
        messages_per_stream,
        elapsed
    );
}

#[tokio::test]
async fn test_large_buffer_capacity() {
    // Test with buf_size = 1000+
    let buffer_size = 1500;
    let message_count = 2000; // More messages than buffer to test overflow

    // Create streams with gaps that will fill buffers
    let stream_a: Vec<u64> = (0..message_count).map(|i| i * 10).collect(); // Every 10ms
    let stream_b: Vec<u64> = (500..message_count + 500).map(|i| i * 10).collect(); // 5s delay

    let stream = StreamBuilder::new()
        .add_messages("A", &stream_a)
        .add_messages("B", &stream_b)
        .build();

    let config = Config::basic(Some(Duration::from_millis(100)), None, buffer_size);

    let start_time = Instant::now();
    let groups = run_sync(stream, ["A", "B"], config).await.unwrap();
    let elapsed = start_time.elapsed();

    // Large buffers should allow more synchronization opportunities
    assert!(!groups.is_empty());

    assert_groups_valid(&groups, Duration::from_millis(100));
    assert_timestamp_ordering(&groups);

    println!(
        "Large buffer: {} groups with buffer_size={} in {:?}",
        groups.len(),
        buffer_size,
        elapsed
    );
}

#[tokio::test]
async fn test_extreme_timestamps() {
    // Test with very large Duration values (near u64::MAX nanoseconds)
    let large_base = 1_000_000_000_000u64; // ~16 minutes in milliseconds
    let timestamps = [
        large_base,
        large_base + 100,
        large_base + 200,
        large_base + 300,
        large_base + 400,
    ];

    let stream = StreamBuilder::new()
        .add_messages("A", &timestamps)
        .add_messages("B", &timestamps.iter().map(|&t| t + 50).collect::<Vec<_>>())
        .build();

    let config = config_with_window(200);
    let groups = run_sync(stream, ["A", "B"], config).await.unwrap();

    // Should handle extreme timestamps correctly (may not form all possible groups)
    assert!(!groups.is_empty());
    assert!(groups.len() <= timestamps.len());

    for group in &groups {
        assert_eq!(group.len(), 2);
    }

    assert_groups_valid(&groups, Duration::from_millis(200));
    assert_timestamp_ordering(&groups);

    // Verify actual extreme values
    if !groups.is_empty() {
        let actual_millis = groups[0]["A"].timestamp().as_millis();
        println!(
            "Extreme timestamps: first group A = {}ms (expected >= 1T)",
            actual_millis
        );
        assert!(
            actual_millis >= 1_000_000_000_000,
            "Expected >= 1T ms, got {}",
            actual_millis
        );
    }

    println!(
        "Extreme timestamps: {} groups with timestamps > 1T ms",
        groups.len()
    );
}

#[tokio::test]
async fn test_rapid_state_changes() {
    // Create a pattern that causes frequent buffer state transitions
    let builder = StreamBuilder::new();

    // Alternating burst pattern to force rapid state changes
    let mut timestamps_a = Vec::new();
    let mut timestamps_b = Vec::new();
    let mut timestamps_c = Vec::new();

    for cycle in 0..50 {
        let base = cycle * 200;

        // Burst phase: all streams active
        for i in 0..5 {
            timestamps_a.push(base + i * 10);
            timestamps_b.push(base + i * 10 + 3);
            timestamps_c.push(base + i * 10 + 7);
        }

        // Sparse phase: only one stream active
        timestamps_a.push(base + 100);
    }

    let stream = builder
        .add_messages("A", &timestamps_a)
        .add_messages("B", &timestamps_b)
        .add_messages("C", &timestamps_c)
        .build();

    let config = Config::basic(Some(Duration::from_millis(50)), None, 8); // Small buffer to force rapid state changes

    let start_time = Instant::now();
    let groups = run_sync(stream, ["A", "B", "C"], config).await.unwrap();
    let elapsed = start_time.elapsed();

    // Should handle rapid state transitions gracefully
    assert!(!groups.is_empty());

    assert_groups_valid(&groups, Duration::from_millis(50));
    assert_timestamp_ordering(&groups);

    println!(
        "Rapid state changes: {} groups from {} cycles in {:?}",
        groups.len(),
        50,
        elapsed
    );
}

#[tokio::test]
async fn test_memory_efficiency() {
    // Test memory usage with sustained high-throughput processing
    let streams = 5;
    let _messages_per_stream = 5000;
    let batch_size = 1000;

    for batch in 0..5 {
        let mut builder = StreamBuilder::new();

        // Use static keys for consistency
        let keys = ["m0", "m1", "m2", "m3", "m4"];
        let key_slice = &keys[..streams];

        for (stream_id, &key) in key_slice.iter().enumerate() {
            let start_time = batch * batch_size;
            let timestamps: Vec<u64> = (0..batch_size)
                .map(|i| ((start_time + i) * 10 + stream_id * 2) as u64)
                .collect();

            builder = builder.add_messages(key, &timestamps);
        }
        let stream = builder.build();

        let config = Config::basic(Some(Duration::from_millis(100)), None, 100); // Reasonable buffer size

        let start_time = Instant::now();
        let groups = run_sync(stream, key_slice.to_vec(), config).await.unwrap();
        let elapsed = start_time.elapsed();

        // Each batch should process efficiently
        assert!(!groups.is_empty());
        assert!(
            elapsed.as_millis() < 1000,
            "Batch {} too slow: {:?}",
            batch,
            elapsed
        );

        assert_groups_valid(&groups, Duration::from_millis(100));
        assert_timestamp_ordering(&groups);

        println!(
            "Memory efficiency batch {}: {} groups in {:?}",
            batch,
            groups.len(),
            elapsed
        );
    }
}

#[tokio::test]
async fn test_performance_benchmarking() {
    // Comprehensive performance test across different configurations
    let test_configs = [
        ("small", 100, 3, 50),    // 100 msgs, 3 streams, 50ms window
        ("medium", 1000, 5, 100), // 1k msgs, 5 streams, 100ms window
        ("large", 10000, 8, 200), // 10k msgs, 8 streams, 200ms window
    ];

    for &(name, msg_count, stream_count, window_ms) in &test_configs {
        let mut builder = StreamBuilder::new();

        // Use static str slices for keys to avoid lifetime issues
        let keys: Vec<&str> = match stream_count {
            3 => vec!["s0", "s1", "s2"],
            5 => vec!["s0", "s1", "s2", "s3", "s4"],
            8 => vec!["s0", "s1", "s2", "s3", "s4", "s5", "s6", "s7"],
            _ => panic!("Unsupported stream count: {}", stream_count),
        };

        for (stream_id, &key) in keys.iter().enumerate() {
            let timestamps: Vec<u64> = (0..msg_count)
                .map(|i| (i * 20 + stream_id * 3) as u64) // 20ms interval, 3ms offset
                .collect();

            builder = builder.add_messages(key, &timestamps);
        }

        let stream = builder.build();

        let config = config_with_window(window_ms);

        let start_time = Instant::now();
        let groups = run_sync(stream, keys.clone(), config).await.unwrap();
        let elapsed = start_time.elapsed();

        let throughput = (msg_count * stream_count) as f64 / elapsed.as_secs_f64();

        // Performance assertions
        assert!(!groups.is_empty());
        assert!(
            throughput > 1000.0,
            "Throughput too low for {}: {:.0} msgs/sec",
            name,
            throughput
        );

        assert_groups_valid(&groups, Duration::from_millis(window_ms));
        assert_timestamp_ordering(&groups);

        println!(
            "Benchmark {}: {} groups, {:.0} msgs/sec, {:?}",
            name,
            groups.len(),
            throughput,
            elapsed
        );
    }
}
