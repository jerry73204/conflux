mod common;

use common::*;
use conflux_core::Config;
use rand::{Rng, SeedableRng, rngs::StdRng};
use std::time::Duration;

#[tokio::test]
async fn test_sensor_data_simulation() {
    // Simulate real sensor timing patterns with jitter
    // Camera: 30fps (33.33ms intervals) ± 2ms jitter
    // Lidar: 10Hz (100ms intervals) ± 5ms jitter
    // GPS: 1Hz (1000ms intervals) ± 10ms jitter

    let mut rng = StdRng::seed_from_u64(42); // Deterministic for testing
    let duration_ms = 2000; // 2 second simulation

    let mut camera_timestamps = Vec::new();
    let mut lidar_timestamps = Vec::new();
    let mut gps_timestamps = Vec::new();

    // Generate camera timestamps (30fps with ±2ms jitter)
    let mut t = 0.0;
    while t < duration_ms as f64 {
        let jitter = rng.random_range(-2.0..=2.0);
        camera_timestamps.push((t + jitter) as u64);
        t += 33.33; // ~30fps
    }

    // Generate lidar timestamps (10Hz with ±5ms jitter)
    t = 0.0;
    while t < duration_ms as f64 {
        let jitter = rng.random_range(-5.0..=5.0);
        lidar_timestamps.push((t + jitter) as u64);
        t += 100.0; // 10Hz
    }

    // Generate GPS timestamps (1Hz with ±10ms jitter)
    t = 0.0;
    while t < duration_ms as f64 {
        let jitter = rng.random_range(-10.0..=10.0);
        gps_timestamps.push((t + jitter) as u64);
        t += 1000.0; // 1Hz
    }

    let stream = StreamBuilder::new()
        .add_messages("camera", &camera_timestamps)
        .add_messages("lidar", &lidar_timestamps)
        .add_messages("gps", &gps_timestamps)
        .build();

    let config = config_with_window(50); // 50ms synchronization window
    let groups = run_sync(stream, ["camera", "lidar", "gps"], config)
        .await
        .unwrap();

    // Should form some synchronized groups despite jitter
    assert!(!groups.is_empty());

    // Each group should have messages within the window
    assert_groups_valid(&groups, Duration::from_millis(50));
    assert_timestamp_ordering(&groups);

    // Log some statistics for analysis
    println!(
        "Sensor simulation: {} groups formed from {} camera, {} lidar, {} GPS messages",
        groups.len(),
        camera_timestamps.len(),
        lidar_timestamps.len(),
        gps_timestamps.len()
    );
}

#[tokio::test]
async fn test_network_delay_simulation() {
    // Simulate variable network delays affecting stream arrival times
    let base_timestamps = [100, 200, 300, 400, 500, 600, 700, 800, 900, 1000];

    // Stream A: Local (no delay)
    let stream_a_times = base_timestamps.to_vec();

    // Stream B: Consistent 20ms network delay
    let stream_b_times: Vec<u64> = base_timestamps.iter().map(|&t| t + 20).collect();

    // Stream C: Variable network delay (10-50ms)
    let mut rng = StdRng::seed_from_u64(123);
    let stream_c_times: Vec<u64> = base_timestamps
        .iter()
        .map(|&t| t + rng.random_range(10..=50))
        .collect();

    let stream = StreamBuilder::new()
        .add_messages("local", &stream_a_times)
        .add_messages("consistent_delay", &stream_b_times)
        .add_messages("variable_delay", &stream_c_times)
        .build();

    let config = config_with_window(100); // Generous window for network delays
    let groups = run_sync(
        stream,
        ["local", "consistent_delay", "variable_delay"],
        config,
    )
    .await
    .unwrap();

    // Should handle network delays gracefully
    assert!(!groups.is_empty());

    // Verify synchronization despite delays
    assert_groups_valid(&groups, Duration::from_millis(100));
    assert_timestamp_ordering(&groups);

    println!("Network delay simulation: {} groups formed", groups.len());
}

#[tokio::test]
async fn test_clock_skew_simulation() {
    // Simulate streams with slightly different time bases (clock skew)
    let base_times = [1000, 2000, 3000, 4000, 5000];

    // Stream A: Reference clock
    let stream_a = base_times.to_vec();

    // Stream B: 0.1% fast clock (1ms per second)
    let stream_b: Vec<u64> = base_times
        .iter()
        .map(|&t| t + (t as f64 * 0.001) as u64)
        .collect();

    // Stream C: 0.1% slow clock
    let stream_c: Vec<u64> = base_times
        .iter()
        .map(|&t| t - (t as f64 * 0.001) as u64)
        .collect();

    let stream = StreamBuilder::new()
        .add_messages("reference", &stream_a)
        .add_messages("fast_clock", &stream_b)
        .add_messages("slow_clock", &stream_c)
        .build();

    let config = config_with_window(50); // Window should handle small clock skews
    let groups = run_sync(stream, ["reference", "fast_clock", "slow_clock"], config)
        .await
        .unwrap();

    // Should synchronize despite clock skew
    assert!(!groups.is_empty());

    assert_groups_valid(&groups, Duration::from_millis(50));
    assert_timestamp_ordering(&groups);

    println!("Clock skew simulation: {} groups formed", groups.len());
}

#[tokio::test]
async fn test_burst_traffic_simulation() {
    // Simulate periods of high message density followed by quiet periods
    let mut timestamps = Vec::new();

    // Burst 1: High density at start (10 messages in 50ms)
    for i in 0..10 {
        timestamps.push(100 + i * 5);
    }

    // Quiet period: 500ms gap

    // Burst 2: Another high density period
    for i in 0..15 {
        timestamps.push(650 + i * 3);
    }

    // Another quiet period

    // Burst 3: Final burst
    for i in 0..8 {
        timestamps.push(1200 + i * 8);
    }

    let stream = StreamBuilder::new()
        .add_messages("sensor_a", &timestamps)
        .add_messages(
            "sensor_b",
            &timestamps.iter().map(|&t| t + 10).collect::<Vec<_>>(),
        )
        .build();

    let config = Config {
        window_size: Duration::from_millis(100),
        start_time: None,
        buf_size: 20, // Larger buffer to handle bursts
        staleness_config: None,
    };

    let groups = run_sync(stream, ["sensor_a", "sensor_b"], config)
        .await
        .unwrap();

    // Should handle burst traffic without losing synchronization
    assert!(!groups.is_empty());

    assert_groups_valid(&groups, Duration::from_millis(100));
    assert_timestamp_ordering(&groups);

    println!(
        "Burst traffic simulation: {} groups from {} total messages",
        groups.len(),
        timestamps.len()
    );
}

#[tokio::test]
async fn test_mixed_frequency_streams() {
    // Real-world scenario: streams with different natural frequencies
    let duration = 5000u64; // 5 seconds

    // High frequency stream (50Hz)
    let high_freq: Vec<u64> = (0..250).map(|i| i * 20).filter(|&t| t < duration).collect();

    // Medium frequency stream (10Hz)
    let med_freq: Vec<u64> = (0..50).map(|i| i * 100).filter(|&t| t < duration).collect();

    // Low frequency stream (2Hz)
    let low_freq: Vec<u64> = (0..10).map(|i| i * 500).filter(|&t| t < duration).collect();

    let stream = StreamBuilder::new()
        .add_messages("high_freq", &high_freq)
        .add_messages("med_freq", &med_freq)
        .add_messages("low_freq", &low_freq)
        .build();

    let config = config_with_window(150); // Window to accommodate frequency differences
    let groups = run_sync(stream, ["high_freq", "med_freq", "low_freq"], config)
        .await
        .unwrap();

    // Mixed frequencies should still allow synchronization
    assert!(!groups.is_empty());

    assert_groups_valid(&groups, Duration::from_millis(150));
    assert_timestamp_ordering(&groups);

    println!(
        "Mixed frequency: {} groups from {}/{}/{} messages at 50Hz/10Hz/2Hz",
        groups.len(),
        high_freq.len(),
        med_freq.len(),
        low_freq.len()
    );
}
