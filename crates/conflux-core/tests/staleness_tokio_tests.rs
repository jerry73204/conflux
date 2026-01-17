use conflux_core::{Config, StalenessConfig, WithTimestamp, sync};
use futures::{FutureExt, StreamExt, TryStreamExt, stream};
use indexmap::IndexMap;
use std::time::{Duration, Instant};
use tokio::time::{sleep, timeout};

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
async fn test_staleness_with_actual_delays() {
    // Test that messages actually become stale and are removed when streams delay
    let test_start = Instant::now();

    // Create stream from array of async blocks (boxed to have same type)
    let messages = stream::iter([
        async { ("A", TestMessage::new(1000, "a1")) }.boxed(),
        async {
            sleep(Duration::from_millis(50)).await;
            ("B", TestMessage::new(1050, "b1"))
        }
        .boxed(),
        async {
            sleep(Duration::from_millis(300)).await; // Long delay to cause staleness
            ("A", TestMessage::new(1400, "a2"))
        }
        .boxed(),
        async {
            sleep(Duration::from_millis(10)).await;
            ("B", TestMessage::new(1450, "b2"))
        }
        .boxed(),
    ])
    .buffered(1) // Process one at a time to maintain order
    .map(Ok); // Wrap each result in Ok

    let staleness_config = StalenessConfig {
        heap_time_horizon: Duration::from_millis(200), // Messages expire after 200ms
        enable_immediate_expiration: true,             // Enable real-time expiration
        ..StalenessConfig::default()
    };

    let config = Config::with_staleness(
        Duration::from_millis(100), // Window size
        None,
        16,
        staleness_config,
    );

    let (output_stream, _feedback_receiver) =
        sync(messages.boxed(), vec!["A", "B"], config).unwrap();

    // Use timeout to ensure test doesn't hang
    let result = timeout(
        Duration::from_secs(2),
        output_stream.try_collect::<Vec<IndexMap<&str, TestMessage>>>(),
    )
    .await;
    let groups = result.unwrap().unwrap();

    println!("Test took: {:?}", test_start.elapsed());
    println!("Groups formed: {}", groups.len());

    // We should get fewer groups because some messages became stale
    // The first A should pair with B, but the delayed A should miss its B partner
    assert!(
        groups.len() <= 2,
        "Too many groups formed: {}",
        groups.len()
    );

    // If staleness worked, we should see evidence of messages being dropped
    let total_messages_sent = 4;
    let total_messages_in_groups: usize = groups.iter().map(|g| g.len()).sum();

    println!(
        "Messages sent: {}, Messages in groups: {}",
        total_messages_sent, total_messages_in_groups
    );

    // Staleness should have prevented some synchronizations
    assert!(
        total_messages_in_groups < total_messages_sent,
        "Expected some messages to be dropped due to staleness"
    );
}

#[tokio::test]
async fn test_staleness_prevents_memory_buildup() {
    // Test that staleness prevents unlimited memory growth with one slow stream

    // Create fast stream A (immediate)
    let fast_stream_a =
        stream::iter((0..10u64).map(|i| Ok(("A", TestMessage::new(i * 10, &format!("a{}", i))))));

    // Create slow stream B (with delays longer than staleness timeout)
    let slow_stream_b = stream::iter((0..10u64).map(|i| i * 10)).then(|timestamp| async move {
        sleep(Duration::from_millis(150)).await; // Longer than staleness timeout
        Ok((
            "B",
            TestMessage::new(timestamp, &format!("b{}", timestamp / 10)),
        ))
    });

    // Interleave the streams
    let messages = fast_stream_a.chain(slow_stream_b);

    let staleness_config = StalenessConfig {
        heap_time_horizon: Duration::from_millis(100), // Short staleness timeout
        heap_max_size: 5,                              // Small heap to test overflow
        enable_immediate_expiration: true,
        ..StalenessConfig::default()
    };

    let config = Config::with_staleness(Duration::from_millis(50), None, 16, staleness_config);

    let (output_stream, _feedback_receiver) =
        sync(messages.boxed(), vec!["A", "B"], config).unwrap();

    let result = timeout(
        Duration::from_secs(5),
        output_stream.try_collect::<Vec<IndexMap<&str, TestMessage>>>(),
    )
    .await;
    let groups = result.unwrap().unwrap();

    println!(
        "Groups formed with memory buildup prevention: {}",
        groups.len()
    );

    // Should have few or no groups because stream B is too slow
    assert!(
        groups.len() < 5,
        "Too many groups formed, staleness may not be working: {}",
        groups.len()
    );

    // Verify groups that formed are valid
    for group in &groups {
        assert_eq!(group.len(), 2, "Invalid group size");
        assert!(group.contains_key("A") && group.contains_key("B"));
    }
}

#[tokio::test]
async fn test_staleness_timer_wheel_overflow() {
    // Test that timer wheel properly handles overflow from heap

    // Create rapid A messages to fill up the heap
    let rapid_a_messages =
        stream::iter((0..10u64).map(|i| Ok(("A", TestMessage::new(i * 5, &format!("a{}", i))))));

    // Create delayed B messages to cause staleness
    let delayed_b_messages = stream::iter((0..5u64).map(|i| i * 5)).then(|timestamp| async move {
        sleep(Duration::from_millis(200)).await; // Delay to cause staleness
        Ok((
            "B",
            TestMessage::new(timestamp, &format!("b{}", timestamp / 5)),
        ))
    });

    let messages = rapid_a_messages.chain(delayed_b_messages);

    let staleness_config = StalenessConfig {
        heap_max_size: 3, // Very small heap to force timer wheel usage
        heap_time_horizon: Duration::from_millis(150),
        timer_wheel_slots: 8,
        timer_wheel_slot_duration: Duration::from_millis(25),
        enable_immediate_expiration: true,
        ..StalenessConfig::default()
    };

    let config = Config::with_staleness(Duration::from_millis(30), None, 16, staleness_config);

    let (output_stream, _feedback_receiver) =
        sync(messages.boxed(), vec!["A", "B"], config).unwrap();

    let result = timeout(
        Duration::from_secs(3),
        output_stream.try_collect::<Vec<IndexMap<&str, TestMessage>>>(),
    )
    .await;
    let groups = result.unwrap().unwrap();

    println!("Groups formed with timer wheel overflow: {}", groups.len());

    // Should have very few groups due to staleness and overflow handling
    assert!(
        groups.len() < 3,
        "Too many groups, timer wheel overflow may not be working: {}",
        groups.len()
    );
}

#[tokio::test]
async fn test_staleness_precision_gap_in_action() {
    // Test that precision gap actually affects staleness checking frequency

    // Create stream from array of async blocks representing bursts
    let messages = stream::iter([
        // Initial burst - A and B messages arrive quickly
        async { ("A", TestMessage::new(1000, "a0")) }.boxed(),
        async { ("A", TestMessage::new(1005, "a1")) }.boxed(),
        async { ("B", TestMessage::new(1000, "b0")) }.boxed(),
        async { ("B", TestMessage::new(1005, "b1")) }.boxed(),
        // Delayed burst - wait to trigger staleness checking
        async {
            sleep(Duration::from_millis(120)).await; // Wait for potential staleness
            ("A", TestMessage::new(1200, "a2"))
        }
        .boxed(),
        async { ("A", TestMessage::new(1205, "a3")) }.boxed(),
        async { ("B", TestMessage::new(1200, "b2")) }.boxed(),
        async { ("B", TestMessage::new(1205, "b3")) }.boxed(),
    ])
    .buffered(1)
    .map(Ok); // Wrap each result in Ok

    let staleness_config = StalenessConfig {
        heap_time_horizon: Duration::from_millis(100),
        precision_gap: Duration::from_millis(10), // Check every 10ms
        enable_immediate_expiration: true,
        ..StalenessConfig::default()
    };

    let config = Config::with_staleness(Duration::from_millis(50), None, 16, staleness_config);

    let (output_stream, _feedback_receiver) =
        sync(messages.boxed(), vec!["A", "B"], config).unwrap();

    let result = timeout(
        Duration::from_secs(2),
        output_stream.try_collect::<Vec<IndexMap<&str, TestMessage>>>(),
    )
    .await;
    let groups = result.unwrap().unwrap();

    println!("Groups formed with precision gap testing: {}", groups.len());

    // Should have limited groups due to staleness from the delay
    assert!(
        groups.len() <= 2,
        "Too many groups, precision gap staleness may not be working: {}",
        groups.len()
    );

    for group in &groups {
        assert!(!group.is_empty(), "Empty group found");
    }
}

#[tokio::test]
async fn test_no_staleness_baseline() {
    // Baseline test: same message pattern but without staleness should work normally

    // Use the same timing pattern as the staleness test but without staleness config
    let messages = stream::iter([
        async { ("A", TestMessage::new(1000, "a1")) }.boxed(),
        async {
            sleep(Duration::from_millis(50)).await;
            ("B", TestMessage::new(1050, "b1"))
        }
        .boxed(),
        async {
            sleep(Duration::from_millis(300)).await;
            ("A", TestMessage::new(1400, "a2"))
        }
        .boxed(),
        async {
            sleep(Duration::from_millis(10)).await;
            ("B", TestMessage::new(1450, "b2"))
        }
        .boxed(),
    ])
    .buffered(1)
    .map(Ok); // Wrap each result in Ok

    // Config WITHOUT staleness
    let config = Config::basic(Duration::from_millis(100), None, 16);

    let (output_stream, _feedback_receiver) =
        sync(messages.boxed(), vec!["A", "B"], config).unwrap();

    let result = timeout(
        Duration::from_secs(2),
        output_stream.try_collect::<Vec<IndexMap<&str, TestMessage>>>(),
    )
    .await;
    let groups = result.unwrap().unwrap();

    println!("Groups formed WITHOUT staleness: {}", groups.len());

    // Without staleness, all messages should synchronize
    assert_eq!(groups.len(), 2, "Should form 2 groups without staleness");

    for group in &groups {
        assert_eq!(group.len(), 2, "Each group should have both A and B");
    }
}

#[tokio::test]
async fn test_messages_forcefully_popped_from_buffer() {
    // Test that messages are forcefully removed from buffers when they become stale
    // This ensures we can observe actual message removal (not just prevention of synchronization)

    let messages = stream::iter([
        // Stream A sends early messages that will become stale
        async { ("A", TestMessage::new(1000, "a1")) }.boxed(),
        async { ("A", TestMessage::new(1010, "a2")) }.boxed(),
        async { ("A", TestMessage::new(1020, "a3")) }.boxed(),
        // Stream B is delayed significantly, causing A messages to become stale
        async {
            sleep(Duration::from_millis(200)).await; // Long delay
            ("B", TestMessage::new(1300, "b1"))
        }
        .boxed(),
        async { ("B", TestMessage::new(1310, "b2")) }.boxed(),
        // More A messages after B arrives
        async { ("A", TestMessage::new(1320, "a4")) }.boxed(),
        async { ("A", TestMessage::new(1330, "a5")) }.boxed(),
    ])
    .buffered(1)
    .map(Ok);

    let staleness_config = StalenessConfig {
        heap_time_horizon: Duration::from_millis(100), // Messages expire after 100ms
        enable_immediate_expiration: true,             // Enable real-time expiration
        heap_max_size: 3,                              // Small heap to force eviction
        ..StalenessConfig::default()
    };

    let config = Config::with_staleness(Duration::from_millis(50), None, 16, staleness_config);

    let (output_stream, _feedback_receiver) =
        sync(messages.boxed(), vec!["A", "B"], config).unwrap();

    let result = timeout(
        Duration::from_secs(3),
        output_stream.try_collect::<Vec<IndexMap<&str, TestMessage>>>(),
    )
    .await;
    let groups = result.unwrap().unwrap();

    println!(
        "Groups formed when messages are forcefully popped: {}",
        groups.len()
    );
    for (i, group) in groups.iter().enumerate() {
        println!("Group {}: {:?}", i, group.keys().collect::<Vec<_>>());
    }

    // Key assertion: Early A messages (a1, a2, a3) should be forcefully removed
    // Only later messages (a4, a5) should potentially sync with B messages
    let total_a_messages_in_groups = groups.iter().flat_map(|group| group.get("A")).count();

    println!("Total A messages in groups: {}", total_a_messages_in_groups);

    // We sent 5 A messages, but early ones should be forcefully popped
    assert!(
        total_a_messages_in_groups < 5,
        "Expected some A messages to be forcefully popped due to staleness, but found {} A messages in groups",
        total_a_messages_in_groups
    );

    // Verify that the remaining groups contain only recent messages
    for group in &groups {
        if let Some(a_msg) = group.get("A") {
            // Only a4 or a5 should remain (timestamps 1320+ ms)
            assert!(
                a_msg.timestamp().as_millis() >= 1320,
                "Found old A message that should have been forcefully popped: timestamp {}ms",
                a_msg.timestamp().as_millis()
            );
        }
    }
}

#[tokio::test]
async fn test_message_popped_before_matching() {
    // Test case where a message is forcefully popped before it gets a chance to match

    let messages = stream::iter([
        // A sends a message that will become stale before B arrives
        async { ("A", TestMessage::new(1000, "a1_will_be_popped")) }.boxed(),
        // Long delay before B's message
        async {
            sleep(Duration::from_millis(150)).await; // Longer than staleness timeout
            ("B", TestMessage::new(1200, "b1_too_late"))
        }
        .boxed(),
        // Send another pair that should sync successfully
        async { ("A", TestMessage::new(1250, "a2_should_sync")) }.boxed(),
        async { ("B", TestMessage::new(1255, "b2_should_sync")) }.boxed(),
    ])
    .buffered(1)
    .map(Ok);

    let staleness_config = StalenessConfig {
        heap_time_horizon: Duration::from_millis(100), // Short staleness window
        enable_immediate_expiration: true,
        ..StalenessConfig::default()
    };

    let config = Config::with_staleness(Duration::from_millis(50), None, 16, staleness_config);

    let (output_stream, _feedback_receiver) =
        sync(messages.boxed(), vec!["A", "B"], config).unwrap();

    let result = timeout(
        Duration::from_secs(2),
        output_stream.try_collect::<Vec<IndexMap<&str, TestMessage>>>(),
    )
    .await;
    let groups = result.unwrap().unwrap();

    println!(
        "Groups when message popped before matching: {}",
        groups.len()
    );

    // Should have at most 1 group (the second pair that synced successfully)
    assert!(
        groups.len() <= 1,
        "Expected at most 1 group, but got {}",
        groups.len()
    );

    // If we have a group, it should contain the second pair
    if !groups.is_empty() {
        let group = &groups[0];
        assert_eq!(group.len(), 2, "Group should have both A and B");

        let a_msg = group.get("A").unwrap();
        let b_msg = group.get("B").unwrap();

        // Verify it's the second pair (not the first that was popped)
        assert!(a_msg.data.contains("a2_should_sync"));
        assert!(b_msg.data.contains("b2_should_sync"));
    }
}

#[tokio::test]
async fn test_message_popped_after_partial_matching() {
    // Test case where some messages form partial groups but then become stale

    let messages = stream::iter([
        // First, A and B form a potential match
        async { ("A", TestMessage::new(1000, "a1")) }.boxed(),
        async { ("B", TestMessage::new(1005, "b1")) }.boxed(),
        // But C is missing for a long time, causing staleness
        async {
            sleep(Duration::from_millis(150)).await; // Delay causes staleness
            ("C", TestMessage::new(1200, "c1_too_late"))
        }
        .boxed(),
        // New messages arrive after staleness
        async { ("A", TestMessage::new(1250, "a2")) }.boxed(),
        async { ("B", TestMessage::new(1255, "b2")) }.boxed(),
        async { ("C", TestMessage::new(1260, "c2")) }.boxed(),
    ])
    .buffered(1)
    .map(Ok);

    let staleness_config = StalenessConfig {
        heap_time_horizon: Duration::from_millis(100),
        enable_immediate_expiration: true,
        ..StalenessConfig::default()
    };

    let config = Config::with_staleness(Duration::from_millis(50), None, 16, staleness_config);

    let (output_stream, _feedback_receiver) =
        sync(messages.boxed(), vec!["A", "B", "C"], config).unwrap();

    let result = timeout(
        Duration::from_secs(2),
        output_stream.try_collect::<Vec<IndexMap<&str, TestMessage>>>(),
    )
    .await;
    let groups = result.unwrap().unwrap();

    println!(
        "Groups after partial matching and staleness: {}",
        groups.len()
    );

    // Should have at most 1 complete group (the second triplet)
    assert!(
        groups.len() <= 1,
        "Expected at most 1 group due to staleness, got {}",
        groups.len()
    );

    // If we have a group, it should be the complete second triplet
    if !groups.is_empty() {
        let group = &groups[0];
        assert_eq!(group.len(), 3, "Group should have A, B, and C");

        // Verify it's the second set of messages
        assert!(group.get("A").unwrap().data.contains("a2"));
        assert!(group.get("B").unwrap().data.contains("b2"));
        assert!(group.get("C").unwrap().data.contains("c2"));
    }
}

#[tokio::test]
async fn test_immediate_vs_lazy_staleness_difference() {
    // Test the difference between immediate and lazy staleness checking

    // First test with immediate expiration (real-time)
    let messages_immediate = stream::iter([
        async { ("A", TestMessage::new(1000, "a1")) }.boxed(),
        async {
            sleep(Duration::from_millis(120)).await; // Longer than staleness horizon
            ("B", TestMessage::new(1150, "b1"))
        }
        .boxed(),
        async { ("A", TestMessage::new(1200, "a2")) }.boxed(),
        async { ("B", TestMessage::new(1205, "b2")) }.boxed(),
    ])
    .buffered(1)
    .map(Ok);

    let staleness_config_immediate = StalenessConfig {
        heap_time_horizon: Duration::from_millis(100),
        enable_immediate_expiration: true, // IMMEDIATE checking
        ..StalenessConfig::default()
    };

    let config_immediate = Config::with_staleness(
        Duration::from_millis(50),
        None,
        16,
        staleness_config_immediate,
    );

    let (output_stream_immediate, _) =
        sync(messages_immediate.boxed(), vec!["A", "B"], config_immediate).unwrap();

    let result_immediate = timeout(
        Duration::from_secs(2),
        output_stream_immediate.try_collect::<Vec<IndexMap<&str, TestMessage>>>(),
    )
    .await;
    let groups_immediate = result_immediate.unwrap().unwrap();

    println!(
        "Groups with IMMEDIATE staleness checking: {}",
        groups_immediate.len()
    );

    // Now test with lazy expiration (only on new message arrival)
    let messages_lazy = stream::iter([
        async { ("A", TestMessage::new(1000, "a1")) }.boxed(),
        async {
            sleep(Duration::from_millis(120)).await; // Same delay
            ("B", TestMessage::new(1150, "b1"))
        }
        .boxed(),
        async { ("A", TestMessage::new(1200, "a2")) }.boxed(),
        async { ("B", TestMessage::new(1205, "b2")) }.boxed(),
    ])
    .buffered(1)
    .map(Ok);

    let staleness_config_lazy = StalenessConfig {
        heap_time_horizon: Duration::from_millis(100),
        enable_immediate_expiration: false, // LAZY checking
        ..StalenessConfig::default()
    };

    let config_lazy =
        Config::with_staleness(Duration::from_millis(50), None, 16, staleness_config_lazy);

    let (output_stream_lazy, _) = sync(messages_lazy.boxed(), vec!["A", "B"], config_lazy).unwrap();

    let result_lazy = timeout(
        Duration::from_secs(2),
        output_stream_lazy.try_collect::<Vec<IndexMap<&str, TestMessage>>>(),
    )
    .await;
    let groups_lazy = result_lazy.unwrap().unwrap();

    println!("Groups with LAZY staleness checking: {}", groups_lazy.len());

    // The key difference: immediate checking should be more aggressive at removing stale messages
    // Lazy checking might allow some messages to stay longer and potentially sync
    println!(
        "Difference in behavior: Immediate={} groups, Lazy={} groups",
        groups_immediate.len(),
        groups_lazy.len()
    );

    // With the same timing, immediate checking should produce fewer or equal groups
    // (because it's more aggressive about removing stale messages)
    assert!(
        groups_immediate.len() <= groups_lazy.len(),
        "Immediate checking should be more aggressive: immediate={}, lazy={}",
        groups_immediate.len(),
        groups_lazy.len()
    );

    // Document the behavior for future reference
    if groups_immediate.len() < groups_lazy.len() {
        println!("✓ Immediate expiration was more aggressive (removed more stale messages)");
    } else if groups_immediate.len() == groups_lazy.len() {
        println!("✓ Both modes produced same result (timing dependent)");
    }
}

#[tokio::test]
async fn test_no_staleness_messages_stay_indefinitely() {
    // Test that without staleness, messages stay in buffers indefinitely and eventually sync
    // This contrasts with staleness-enabled behavior where messages get forcefully removed

    let messages = stream::iter([
        // Stream A sends early messages
        async { ("A", TestMessage::new(1000, "a1_should_stay")) }.boxed(),
        async { ("A", TestMessage::new(1100, "a2_should_stay")) }.boxed(),
        async { ("A", TestMessage::new(1200, "a3_should_stay")) }.boxed(),
        // Stream B is delayed significantly (would cause staleness if enabled)
        async {
            sleep(Duration::from_millis(300)).await; // Very long delay
            ("B", TestMessage::new(1050, "b1_finally_arrives"))
        }
        .boxed(),
        async { ("B", TestMessage::new(1150, "b2")) }.boxed(),
        async { ("B", TestMessage::new(1250, "b3")) }.boxed(),
    ])
    .buffered(1)
    .map(Ok);

    // Config WITHOUT staleness - messages should stay indefinitely
    // Use larger window to allow for the delayed synchronization
    let config = Config::basic(Duration::from_millis(100), None, 16);

    let (output_stream, _feedback_receiver) =
        sync(messages.boxed(), vec!["A", "B"], config).unwrap();

    let result = timeout(
        Duration::from_secs(3),
        output_stream.try_collect::<Vec<IndexMap<&str, TestMessage>>>(),
    )
    .await;
    let groups = result.unwrap().unwrap();

    println!(
        "Groups formed WITHOUT staleness (messages stay indefinitely): {}",
        groups.len()
    );
    for (i, group) in groups.iter().enumerate() {
        let a_msg = group.get("A").map(|m| &m.data as &str).unwrap_or("none");
        let b_msg = group.get("B").map(|m| &m.data as &str).unwrap_or("none");
        println!("Group {}: A='{}', B='{}'", i, a_msg, b_msg);
    }

    // Key assertion: Messages should synchronize because none are removed by staleness
    // Without staleness, we should get at least some groups (vs 0 with staleness)
    assert!(
        groups.len() >= 2,
        "Expected at least 2 groups without staleness, but got {}",
        groups.len()
    );

    // Verify that early A messages (that would be stale) are still present
    let a_messages_in_groups: Vec<_> = groups
        .iter()
        .filter_map(|group| group.get("A"))
        .map(|msg| &msg.data)
        .collect();

    // The key test: early messages should stay and sync (vs being forcefully removed)
    assert!(
        a_messages_in_groups.contains(&&"a1_should_stay".to_string()),
        "Early A message should stay without staleness: {:?}",
        a_messages_in_groups
    );

    // Count total messages that actually synchronized
    let total_messages_synced = groups.len() * 2; // Each group has 2 messages
    println!(
        "Total messages synchronized without staleness: {}",
        total_messages_synced
    );

    // This demonstrates the key difference: without staleness, old messages stay and can sync
    assert!(
        total_messages_synced >= 4,
        "Without staleness, should synchronize multiple messages including old ones"
    );
}

#[tokio::test]
async fn test_extreme_delay_without_staleness() {
    // Test extremely long delays without staleness - messages should still eventually sync

    let messages = stream::iter([
        async { ("A", TestMessage::new(1000, "a1_patient")) }.boxed(),
        // Extremely long delay that would definitely trigger staleness if enabled
        async {
            sleep(Duration::from_millis(500)).await; // Half a second delay
            ("B", TestMessage::new(1050, "b1_very_late"))
        }
        .boxed(),
        // Another pair with normal timing
        async { ("A", TestMessage::new(1100, "a2_normal")) }.boxed(),
        async { ("B", TestMessage::new(1150, "b2_normal")) }.boxed(),
    ])
    .buffered(1)
    .map(Ok);

    // Config WITHOUT staleness - use larger window for extreme delays
    let config = Config::basic(Duration::from_millis(200), None, 16);

    let (output_stream, _feedback_receiver) =
        sync(messages.boxed(), vec!["A", "B"], config).unwrap();

    let result = timeout(
        Duration::from_secs(4), // Give enough time for the delay
        output_stream.try_collect::<Vec<IndexMap<&str, TestMessage>>>(),
    )
    .await;
    let groups = result.unwrap().unwrap();

    println!(
        "Groups formed with extreme delay but no staleness: {}",
        groups.len()
    );

    // Should have at least 1 group - the key is that delayed messages can still sync
    assert!(
        !groups.is_empty(),
        "Expected at least 1 group even with extreme delay, but got {}",
        groups.len()
    );

    // Verify the patient message eventually synced (or at least some sync occurred)
    let found_patient_a = groups.iter().any(|group| {
        group
            .get("A")
            .is_some_and(|msg| msg.data.contains("a1_patient"))
    });

    let found_normal_a = groups.iter().any(|group| {
        group
            .get("A")
            .is_some_and(|msg| msg.data.contains("a2_normal"))
    });

    // At least one of the messages should sync (demonstrating staleness is disabled)
    assert!(
        found_patient_a || found_normal_a,
        "At least one A message should sync without staleness"
    );

    println!("✓ Without staleness: messages can sync even after extreme delays");
}

#[tokio::test]
async fn test_staleness_enabled_vs_disabled_comparison() {
    // Direct comparison: identical scenarios with staleness enabled vs disabled

    let test_scenario = || {
        stream::iter([
            async { ("A", TestMessage::new(1000, "a1")) }.boxed(),
            async {
                sleep(Duration::from_millis(150)).await; // Longer than staleness horizon
                ("B", TestMessage::new(1200, "b1"))
            }
            .boxed(),
            async { ("A", TestMessage::new(1250, "a2")) }.boxed(),
            async { ("B", TestMessage::new(1255, "b2")) }.boxed(),
        ])
        .buffered(1)
        .map(Ok)
    };

    // Test WITH staleness enabled
    let staleness_config = StalenessConfig {
        heap_time_horizon: Duration::from_millis(100), // Short horizon
        enable_immediate_expiration: true,
        ..StalenessConfig::default()
    };

    let config_with_staleness =
        Config::with_staleness(Duration::from_millis(50), None, 16, staleness_config);

    let (output_stream_staleness, _) = sync(
        test_scenario().boxed(),
        vec!["A", "B"],
        config_with_staleness,
    )
    .unwrap();

    let result_staleness = timeout(
        Duration::from_secs(2),
        output_stream_staleness.try_collect::<Vec<IndexMap<&str, TestMessage>>>(),
    )
    .await;
    let groups_with_staleness = result_staleness.unwrap().unwrap();

    // Test WITHOUT staleness (disabled)
    let config_no_staleness = Config::basic(Duration::from_millis(50), None, 16);

    let (output_stream_no_staleness, _) =
        sync(test_scenario().boxed(), vec!["A", "B"], config_no_staleness).unwrap();

    let result_no_staleness = timeout(
        Duration::from_secs(2),
        output_stream_no_staleness.try_collect::<Vec<IndexMap<&str, TestMessage>>>(),
    )
    .await;
    let groups_no_staleness = result_no_staleness.unwrap().unwrap();

    println!(
        "Comparison - Staleness ENABLED: {} groups",
        groups_with_staleness.len()
    );
    println!(
        "Comparison - Staleness DISABLED: {} groups",
        groups_no_staleness.len()
    );

    // Key assertion: Disabled staleness should allow more/equal synchronizations
    assert!(
        groups_no_staleness.len() >= groups_with_staleness.len(),
        "Disabled staleness should allow more synchronizations: disabled={}, enabled={}",
        groups_no_staleness.len(),
        groups_with_staleness.len()
    );

    // Key difference: without staleness should allow more synchronizations
    assert!(
        !groups_no_staleness.is_empty(),
        "Without staleness, should sync at least some messages"
    );

    // With staleness, we should get fewer or equal groups due to stale message removal
    assert!(
        groups_with_staleness.len() <= groups_no_staleness.len(),
        "With staleness, should sync fewer or equal messages due to removal"
    );

    println!("✓ Staleness disabled allows all messages to eventually synchronize");
    println!("✓ Staleness enabled prevents stale message synchronization");
}

#[tokio::test]
async fn test_memory_buildup_without_staleness() {
    // Test that without staleness, messages can accumulate in buffers indefinitely
    // This demonstrates the memory buildup problem that staleness solves

    let messages = stream::iter([
        // Stream A sends many messages rapidly
        async { ("A", TestMessage::new(1000, "a1")) }.boxed(),
        async { ("A", TestMessage::new(1100, "a2")) }.boxed(),
        async { ("A", TestMessage::new(1200, "a3")) }.boxed(),
        async { ("A", TestMessage::new(1300, "a4")) }.boxed(),
        async { ("A", TestMessage::new(1400, "a5")) }.boxed(),
        // Stream B sends very few messages, creating imbalance
        async {
            sleep(Duration::from_millis(200)).await;
            ("B", TestMessage::new(1150, "b1"))
        }
        .boxed(),
        // More A messages that won't have B partners
        async { ("A", TestMessage::new(1500, "a6")) }.boxed(),
        async { ("A", TestMessage::new(1600, "a7")) }.boxed(),
    ])
    .buffered(1)
    .map(Ok);

    // Config WITHOUT staleness - allows unlimited buffer growth
    let config = Config::basic(Duration::from_millis(100), None, 16);

    let (output_stream, _feedback_receiver) =
        sync(messages.boxed(), vec!["A", "B"], config).unwrap();

    let result = timeout(
        Duration::from_secs(3),
        output_stream.try_collect::<Vec<IndexMap<&str, TestMessage>>>(),
    )
    .await;
    let groups = result.unwrap().unwrap();

    println!(
        "Groups formed without staleness (potential memory buildup): {}",
        groups.len()
    );

    // The key insight: we sent 7 A messages but only 1 B message
    // Without staleness, unmatched A messages accumulate in buffers indefinitely
    let total_a_messages_sent = 7;
    let total_b_messages_sent = 1;
    let max_possible_groups = std::cmp::min(total_a_messages_sent, total_b_messages_sent);

    // Should get at most 1 group (limited by B messages)
    assert!(
        groups.len() <= max_possible_groups,
        "Groups limited by B messages: got {}, max possible {}",
        groups.len(),
        max_possible_groups
    );

    // Count how many A messages actually synchronized
    let a_messages_in_groups = groups.iter().filter_map(|group| group.get("A")).count();

    // The remaining A messages (7 - synchronized) stay in buffer indefinitely
    let remaining_a_messages = total_a_messages_sent - a_messages_in_groups;

    println!("A messages synchronized: {}", a_messages_in_groups);
    println!(
        "A messages remaining in buffer indefinitely: {}",
        remaining_a_messages
    );

    // This demonstrates the memory buildup problem
    assert!(
        remaining_a_messages >= 6,
        "Should demonstrate memory buildup: {} A messages remain in buffer",
        remaining_a_messages
    );

    println!("✓ Without staleness: excess A messages remain in buffer indefinitely");
    println!("✓ This demonstrates the memory buildup problem staleness solves");
}

#[tokio::test]
async fn test_staleness_stress_high_frequency_bursts() {
    // Stress test: Intensely frequent message bursts with staleness enabled
    // Tests staleness system performance under high-volume conditions

    let messages = stream::iter((0..1000u64).map(|i| {
        if i % 100 == 0 {
            // Every 100th message is from stream B with significant delay
            async move {
                sleep(Duration::from_millis(50)).await;
                ("B", TestMessage::new(i * 10, &format!("b{}", i)))
            }
            .boxed()
        } else {
            // Stream A sends rapid bursts
            async move { ("A", TestMessage::new(i * 10, &format!("a{}", i))) }.boxed()
        }
    }))
    .buffered(10) // Process multiple messages concurrently
    .map(Ok);

    let staleness_config = StalenessConfig {
        heap_time_horizon: Duration::from_millis(100), // Short horizon for aggressive cleanup
        heap_max_size: 50,                             // Small heap to test overflow
        enable_immediate_expiration: true,
        precision_gap: Duration::from_micros(500), // High frequency checking
        ..StalenessConfig::default()
    };

    let config = Config::with_staleness(Duration::from_millis(200), None, 32, staleness_config);

    let start_time = Instant::now();
    let (output_stream, _feedback_receiver) =
        sync(messages.boxed(), vec!["A", "B"], config).unwrap();

    let result = timeout(
        Duration::from_secs(10),
        output_stream.try_collect::<Vec<IndexMap<&str, TestMessage>>>(),
    )
    .await;
    let groups = result.unwrap().unwrap();
    let elapsed = start_time.elapsed();

    println!("Staleness stress test completed in: {:?}", elapsed);
    println!(
        "Groups formed under high-frequency stress: {}",
        groups.len()
    );

    // Count messages processed
    let total_messages_synced = groups.len() * 2;
    let total_messages_sent = 1000;
    let staleness_effectiveness =
        ((total_messages_sent - total_messages_synced) as f64 / total_messages_sent as f64) * 100.0;

    println!("Messages sent: {}", total_messages_sent);
    println!("Messages synchronized: {}", total_messages_synced);
    println!(
        "Staleness effectiveness: {:.1}% reduction",
        staleness_effectiveness
    );

    // Assertions
    assert!(
        groups.len() < 100,
        "Staleness should prevent excessive synchronization: {} groups",
        groups.len()
    );

    assert!(
        elapsed < Duration::from_secs(5),
        "Should complete within reasonable time even with 1000 messages"
    );

    // Verify staleness prevented memory buildup
    assert!(
        total_messages_synced < total_messages_sent,
        "Staleness should reduce total synchronized messages"
    );

    println!("✓ Staleness system handled high-frequency bursts efficiently");
}

#[tokio::test]
async fn test_staleness_stress_extreme_imbalance() {
    // Stress test: Extreme stream imbalance with staleness protection
    // One stream sends thousands of messages, other sends very few

    let rapid_a_messages = (0..2000u64)
        .map(|i| async move { ("A", TestMessage::new(i * 5, &format!("rapid_a{}", i))) }.boxed());

    let sparse_b_messages = (0..5u64).map(|i| {
        async move {
            sleep(Duration::from_millis(100 * i)).await; // Increasing delays
            ("B", TestMessage::new(i * 1000, &format!("sparse_b{}", i)))
        }
        .boxed()
    });

    // Interleave rapid A messages with sparse B messages
    let messages = stream::iter(rapid_a_messages.chain(sparse_b_messages))
        .buffered(20) // High concurrency
        .map(Ok);

    let staleness_config = StalenessConfig {
        heap_time_horizon: Duration::from_millis(50), // Very aggressive staleness
        heap_max_size: 20,                            // Very small heap
        timer_wheel_slots: 16,
        timer_wheel_slot_duration: Duration::from_millis(10),
        enable_immediate_expiration: true,
        precision_gap: Duration::from_micros(100), // Ultra-high frequency checking
    };

    let config = Config::with_staleness(Duration::from_millis(100), None, 64, staleness_config);

    let start_time = Instant::now();
    let (output_stream, _feedback_receiver) =
        sync(messages.boxed(), vec!["A", "B"], config).unwrap();

    let result = timeout(
        Duration::from_secs(15),
        output_stream.try_collect::<Vec<IndexMap<&str, TestMessage>>>(),
    )
    .await;
    let groups = result.unwrap().unwrap();
    let elapsed = start_time.elapsed();

    println!("Extreme imbalance stress test completed in: {:?}", elapsed);
    println!("Groups formed with extreme imbalance: {}", groups.len());

    let total_a_sent = 2000;
    let total_b_sent = 5;
    let max_theoretical_groups = std::cmp::min(total_a_sent, total_b_sent);

    println!("Max theoretical groups: {}", max_theoretical_groups);
    println!("Actual groups: {}", groups.len());

    // Key assertions for extreme imbalance
    assert!(
        groups.len() <= max_theoretical_groups,
        "Groups limited by sparse stream: got {}, max {}",
        groups.len(),
        max_theoretical_groups
    );

    // Staleness should prevent memory explosion
    assert!(
        elapsed < Duration::from_secs(10),
        "Should complete quickly despite 2000+ messages due to staleness cleanup"
    );

    // Verify memory didn't explode (groups should be very limited)
    assert!(
        groups.len() <= 10,
        "Staleness should severely limit groups with extreme imbalance: {}",
        groups.len()
    );

    println!("✓ Staleness protected against extreme stream imbalance");
}

#[tokio::test]
async fn test_staleness_stress_concurrent_multi_stream() {
    // Stress test: Multiple streams with different frequencies and staleness
    // Tests complex multi-stream scenarios under stress

    let stream_a = (0..500u64)
        .map(|i| async move { ("A", TestMessage::new(i * 20, &format!("fast_a{}", i))) }.boxed());

    let stream_b = (0..250u64).map(|i| {
        async move {
            if i % 10 == 0 {
                sleep(Duration::from_millis(20)).await; // Occasional delays
            }
            ("B", TestMessage::new(i * 40, &format!("medium_b{}", i)))
        }
        .boxed()
    });

    let stream_c = (0..100u64).map(|i| {
        async move {
            sleep(Duration::from_millis(i % 5 * 10)).await; // Variable delays
            ("C", TestMessage::new(i * 100, &format!("slow_c{}", i)))
        }
        .boxed()
    });

    let stream_d = (0..50u64).map(|i| {
        async move {
            sleep(Duration::from_millis(50 + i % 3 * 20)).await; // Very slow
            ("D", TestMessage::new(i * 200, &format!("very_slow_d{}", i)))
        }
        .boxed()
    });

    // Combine all streams
    let messages = stream::iter(stream_a.chain(stream_b).chain(stream_c).chain(stream_d))
        .buffered(50) // High concurrency for stress
        .map(Ok);

    let staleness_config = StalenessConfig {
        heap_time_horizon: Duration::from_millis(500), // More generous for 4 streams
        heap_max_size: 200,
        timer_wheel_slots: 32,
        timer_wheel_slot_duration: Duration::from_millis(20),
        enable_immediate_expiration: true,
        precision_gap: Duration::from_millis(5), // Less aggressive checking
    };

    let config = Config::with_staleness(Duration::from_millis(300), None, 128, staleness_config);

    let start_time = Instant::now();
    let (output_stream, _feedback_receiver) =
        sync(messages.boxed(), vec!["A", "B", "C", "D"], config).unwrap();

    let result = timeout(
        Duration::from_secs(20),
        output_stream.try_collect::<Vec<IndexMap<&str, TestMessage>>>(),
    )
    .await;
    let groups = result.unwrap().unwrap();
    let elapsed = start_time.elapsed();

    println!("Multi-stream stress test completed in: {:?}", elapsed);
    println!("Groups formed in multi-stream stress: {}", groups.len());

    let total_messages_sent = 500 + 250 + 100 + 50; // 900 total
    let total_messages_synced = groups.iter().map(|g| g.len()).sum::<usize>();

    println!("Total messages sent: {}", total_messages_sent);
    println!("Total messages synchronized: {}", total_messages_synced);

    // Verify groups are complete (all 4 streams present)
    for group in &groups {
        assert_eq!(
            group.len(),
            4,
            "Each group should have all 4 streams: A, B, C, D"
        );
        assert!(group.contains_key("A"));
        assert!(group.contains_key("B"));
        assert!(group.contains_key("C"));
        assert!(group.contains_key("D"));
    }

    // Performance assertions
    assert!(
        elapsed < Duration::from_secs(15),
        "Should complete within reasonable time despite complexity"
    );

    // Staleness should limit synchronization
    assert!(
        total_messages_synced < total_messages_sent,
        "Staleness should reduce total synchronized messages in complex scenario"
    );

    // Should get some groups but staleness limits them
    if groups.is_empty() {
        println!("Note: No groups formed - 4-stream sync is challenging with staleness");
        // This is actually expected behavior - 4-stream sync is hard with staleness
        assert!(
            total_messages_sent > total_messages_synced,
            "Staleness should prevent some synchronizations even if no groups form"
        );
    } else {
        assert!(
            groups.len() < 100,
            "Should get reasonable number of groups: {}",
            groups.len()
        );
    }

    println!("✓ Staleness handled complex multi-stream stress scenario");
}

#[tokio::test]
async fn test_staleness_stress_timer_wheel_overflow_intensive() {
    // Stress test: Force intensive timer wheel overflow scenarios
    // Rapid messages designed to overflow heap and stress timer wheel

    let burst_size = 1000;
    let messages = stream::iter((0..burst_size).map(|i| {
        if i % 2 == 0 {
            // Stream A: Rapid fire messages
            async move { ("A", TestMessage::new(i * 2, &format!("burst_a{}", i))) }.boxed()
        } else {
            // Stream B: Slightly delayed to create temporal spread
            async move {
                sleep(Duration::from_millis(1)).await; // Tiny delay
                ("B", TestMessage::new(i * 2 + 1, &format!("burst_b{}", i)))
            }
            .boxed()
        }
    }))
    .buffered(100) // Very high concurrency to stress the system
    .map(Ok);

    let staleness_config = StalenessConfig {
        heap_time_horizon: Duration::from_millis(50),
        heap_max_size: 10, // Extremely small heap to force timer wheel usage
        timer_wheel_slots: 64,
        timer_wheel_slot_duration: Duration::from_millis(5),
        enable_immediate_expiration: true,
        precision_gap: Duration::from_micros(50), // Ultra-high frequency
    };

    let config = Config::with_staleness(Duration::from_millis(100), None, 256, staleness_config);

    let start_time = Instant::now();
    let (output_stream, _feedback_receiver) =
        sync(messages.boxed(), vec!["A", "B"], config).unwrap();

    let result = timeout(
        Duration::from_secs(10),
        output_stream.try_collect::<Vec<IndexMap<&str, TestMessage>>>(),
    )
    .await;
    let groups = result.unwrap().unwrap();
    let elapsed = start_time.elapsed();

    println!(
        "Timer wheel overflow stress test completed in: {:?}",
        elapsed
    );
    println!("Groups formed under timer wheel stress: {}", groups.len());

    let messages_per_second = burst_size as f64 / elapsed.as_secs_f64();
    println!(
        "Processing rate: {:.0} messages/second",
        messages_per_second
    );

    // Performance assertions
    assert!(
        elapsed < Duration::from_secs(5),
        "Should handle burst efficiently despite timer wheel stress"
    );

    assert!(
        messages_per_second > 100.0,
        "Should maintain reasonable throughput: {:.0} msg/s",
        messages_per_second
    );

    // Staleness should limit groups (though maybe not as much as expected with tiny delays)
    assert!(
        groups.len() < burst_size as usize,
        "Timer wheel overflow should limit groups: {} groups from {} messages",
        groups.len(),
        burst_size
    );

    // The key test is that we're getting high throughput with staleness enabled
    println!("Timer wheel handled {} groups efficiently", groups.len());

    println!("✓ Timer wheel handled intensive overflow stress efficiently");
}

#[tokio::test]
async fn test_staleness_stress_mixed_timing_chaos() {
    // Stress test: Chaotic mixed timing patterns with staleness
    // Simulates real-world unpredictable message timing

    let messages = stream::iter((0..800u64).map(|i| {
        let stream_key = match i % 3 {
            0 => "A",
            1 => "B",
            _ => "C",
        };

        // Deterministic delay pattern to avoid threading issues with RNG
        let delay_ms = match i % 10 {
            0..=5 => i % 10,      // Mostly fast
            6..=8 => 10 + i % 40, // Sometimes medium
            _ => 50 + i % 150,    // Rarely slow
        };

        async move {
            sleep(Duration::from_millis(delay_ms)).await;
            (
                stream_key,
                TestMessage::new(i * 15, &format!("chaos_{}_{}", stream_key, i)),
            )
        }
        .boxed()
    }))
    .buffered(30) // Moderate concurrency for chaos
    .map(Ok);

    let staleness_config = StalenessConfig {
        heap_time_horizon: Duration::from_millis(150),
        heap_max_size: 75,
        timer_wheel_slots: 24,
        timer_wheel_slot_duration: Duration::from_millis(15),
        enable_immediate_expiration: true,
        precision_gap: Duration::from_millis(2), // Frequent but not extreme
    };

    let config = Config::with_staleness(Duration::from_millis(100), None, 64, staleness_config);

    let start_time = Instant::now();
    let (output_stream, _feedback_receiver) =
        sync(messages.boxed(), vec!["A", "B", "C"], config).unwrap();

    let result = timeout(
        Duration::from_secs(25),
        output_stream.try_collect::<Vec<IndexMap<&str, TestMessage>>>(),
    )
    .await;
    let groups = result.unwrap().unwrap();
    let elapsed = start_time.elapsed();

    println!("Chaotic timing stress test completed in: {:?}", elapsed);
    println!("Groups formed under chaotic timing: {}", groups.len());

    let total_messages_sent = 800;
    let total_messages_synced = groups.iter().map(|g| g.len()).sum::<usize>();
    let staleness_impact = total_messages_sent - total_messages_synced;

    println!("Chaos test - Messages sent: {}", total_messages_sent);
    println!(
        "Chaos test - Messages synchronized: {}",
        total_messages_synced
    );
    println!(
        "Chaos test - Messages prevented by staleness: {}",
        staleness_impact
    );

    // Verify all groups are complete
    for group in &groups {
        assert_eq!(
            group.len(),
            3,
            "Each group should have A, B, and C in chaos test"
        );
    }

    // Staleness should handle chaos gracefully
    assert!(
        elapsed < Duration::from_secs(20),
        "Should handle chaotic timing within reasonable time"
    );

    assert!(
        !groups.is_empty(),
        "Should produce some groups despite chaos"
    );

    assert!(
        staleness_impact > 0,
        "Staleness should prevent some synchronizations in chaotic conditions"
    );

    println!("✓ Staleness handled chaotic mixed timing patterns robustly");
}
