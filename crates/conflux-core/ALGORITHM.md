# conflux-core Algorithm

This document provides a detailed explanation of the synchronization algorithm used in the conflux-core library. It's intended for contributors and developers who want to understand the internal workings of the system.

## Core Concepts

### Time Windows

The synchronizer operates on the concept of **time windows** - discrete time periods during which messages from different streams should be grouped together. Each window attempts to collect exactly one message from each configured stream.

```
Timeline:    [----Window 1----][----Window 2----][----Window 3----]
             1000ms    1100ms  1100ms    1200ms  1200ms    1300ms

Stream A:      A1                 A2                 A3
Stream B:        B1                 B2                 B3
Stream C:          C1                 C2                 C3

Groups:      {A1,B1,C1}         {A2,B2,C2}         {A3,B3,C3}
```

### Message Ordering and Timestamps

All messages must implement the `WithTimestamp` trait, providing a monotonic timestamp used for synchronization. The algorithm assumes:

1. **Relative Time Ordering**: Messages within each stream maintain temporal order
2. **Cross-Stream Correlation**: Messages with similar timestamps should be grouped
3. **Bounded Latency**: Messages have implicit or explicit expiration times

## Buffer Management Architecture

### Stream Buffers

Each stream maintains its own message buffer implemented as a `VecDeque` for efficient front/back operations:

```
Stream Buffers:
┌─────────────┐    ┌─────────────┐    ┌─────────────┐
│   Stream A  │    │   Stream B  │    │   Stream C  │
│ [A1][A2][A3]│    │ [B1][B2]    │    │ [C1][C2][C3]│
└─────────────┘    └─────────────┘    └─────────────┘
       │                   │                   │
       └─────────┬─────────┴─────────┬─────────┘
                 │                   │
                 ▼                   ▼
         ┌─────────────────────────────────┐
         │     Synchronization Engine      │
         │   • Window calculation          │
         │   • Message matching            │
         │   • Group formation             │
         └─────────────────────────────────┘
                         │
                         ▼
              ┌─────────────────────┐
              │  Synchronized Group │
              │    {A1, B1, C1}     │
              └─────────────────────┘
```

### Buffer Operations

The `Buffer<T>` struct provides key operations:

```rust
impl<T: WithTimestamp> Buffer<T> {
    // Add new message (always to back)
    fn push_back(&mut self, message: T)

    // Remove oldest message
    fn pop_front(&mut self) -> Option<T>

    // Peek at oldest message without removing
    fn front(&self) -> Option<&T>

    // Remove expired messages based on reference timestamp
    fn drop_expired(&mut self, reference_timestamp: Duration) -> usize

    // Check if buffer is at capacity
    fn is_full(&self) -> bool
}
```

## Synchronization State Machine

### State Structure

The `State<K, T>` struct maintains the synchronization state:

```rust
pub struct State<K, T> {
    // Per-stream message buffers
    buffers: IndexMap<K, Buffer<T>>,

    // Synchronization configuration
    window_size: Duration,
    start_time: Option<Duration>,

    // Optional staleness detection
    staleness_config: Option<StalenessConfig>,
    staleness_detector: Option<StalenessDetector<K, T>>,
}
```

### Core Algorithm: `try_match()`

The heart of the algorithm is the `try_match()` method that attempts to form synchronized groups:

```rust
impl<K, T> State<K, T>
where
    K: Clone + Hash + Eq,
    T: WithTimestamp + Clone,
{
    fn try_match(&mut self) -> Option<IndexMap<K, T>> {
        // 1. Check if all streams have messages
        if !self.all_streams_have_messages() {
            return None;
        }

        // 2. Calculate window boundaries
        let inf_time = self.inf_timestamp();
        let sup_time = self.sup_timestamp();

        // 3. Check if window is valid
        if sup_time.saturating_sub(inf_time) > self.window_size {
            // Window too large - drop oldest messages
            self.drop_old_messages();
            return None;
        }

        // 4. Extract one message from each stream
        let mut group = IndexMap::new();
        for (key, buffer) in &mut self.buffers {
            if let Some(message) = buffer.pop_front() {
                group.insert(key.clone(), message);
            }
        }

        Some(group)
    }
}
```

### Timestamp Calculation

Two key functions determine window boundaries:

```rust
// Earliest timestamp across all streams (window start)
fn inf_timestamp(&self) -> Duration {
    self.buffers
        .values()
        .filter_map(|buf| buf.front())
        .map(|msg| msg.timestamp())
        .min()
        .unwrap_or(Duration::ZERO)
}

// Latest timestamp across all streams (window end)
fn sup_timestamp(&self) -> Duration {
    self.buffers
        .values()
        .filter_map(|buf| buf.front())
        .map(|msg| msg.timestamp())
        .max()
        .unwrap_or(Duration::ZERO)
}
```

### Message Dropping Strategy

When windows become too large, the algorithm drops old messages:

```rust
fn drop_old_messages(&mut self) {
    let inf_time = self.inf_timestamp();

    for buffer in self.buffers.values_mut() {
        // Drop messages that are too far behind
        while let Some(front_msg) = buffer.front() {
            if front_msg.timestamp() == inf_time {
                buffer.pop_front();
                break;
            }
        }
    }
}
```

## Staleness Detection System

### Problem Statement

In real-time scenarios, streams can become imbalanced:

```
High-Frequency Scenario:
Stream A: [A1][A2][A3][A4][A5]... (fast stream)
Stream B: [B1]................... (slow/failed stream)

Without Staleness: Messages A2,A3,A4,A5... accumulate forever
With Staleness:    Messages expire after configured timeout → memory bounded
```

### Hybrid Architecture

The staleness detector uses a two-tier approach for efficient message expiration:

```rust
pub struct StalenessDetector<K, T> {
    // Tier 1: Min-heap for precise near-term expiration
    constrained_heap: ConstrainedHeap<K, T>,

    // Tier 2: Timer wheel for high-frequency overflow
    timer_wheel: TimerWheel<K, T>,

    // Configuration
    config: StalenessConfig,

    // Async task for immediate expiration (tokio feature)
    #[cfg(feature = "tokio")]
    expiration_task: Option<JoinHandle<()>>,
}
```

### Constrained Min-Heap

The min-heap enforces three constraints to maintain bounded performance:

1. **Size Constraint**: Maximum number of entries (e.g., 256)
2. **Temporal Constraint**: Only near-future expirations (e.g., next 100ms)
3. **Precision Gap**: Minimum time between expiration checks (e.g., 500μs)

```rust
impl<K, T> ConstrainedHeap<K, T> {
    fn try_add_expiration(&mut self, expires_at: Instant, key: K, message: T) -> Result<(), (K, T)> {
        let now = Instant::now();

        // Temporal constraint: only near-future
        if expires_at.duration_since(now) > self.time_horizon {
            return Err((key, message)); // Delegate to timer wheel
        }

        // Size constraint: prevent unbounded growth
        if self.heap.len() >= self.max_size {
            return Err((key, message)); // Overflow to timer wheel
        }

        // Precision gap: coalesce messages within same precision window
        if let Some(top) = self.heap.peek() {
            let gap_since_top = expires_at.duration_since(top.expires_at);
            if gap_since_top < self.precision_gap {
                // Reuse existing check time
                self.add_to_existing_check(top.expires_at, key, message);
                return Ok(());
            }
        }

        // All constraints satisfied - add to heap
        self.heap.push(Reverse(ExpirationEntry { expires_at, key, message }));
        Ok(())
    }
}
```

### Timer Wheel for Overflow

Messages that don't fit in the constrained heap are stored in a timer wheel:

```rust
struct TimerWheel<K, T> {
    slots: VecDeque<Vec<(K, T, Instant)>>,
    slot_duration: Duration,      // e.g., 100ms per slot
    total_slots: usize,           // e.g., 100 slots = 10 second horizon
    current_time: Instant,
}

impl<K, T> TimerWheel<K, T> {
    fn add_message(&mut self, key: K, message: T, expires_at: Instant) {
        let slots_from_now = expires_at
            .duration_since(self.current_time)
            .as_nanos() / self.slot_duration.as_nanos();

        if slots_from_now < self.total_slots as u128 {
            let slot_index = slots_from_now as usize;
            self.slots[slot_index].push((key, message, expires_at));
        }
        // Messages beyond wheel horizon are dropped or handled specially
    }
}
```

### Async Expiration Task (Tokio Feature)

When the `tokio` feature is enabled and immediate expiration is requested, a background task provides precise timing:

```rust
#[cfg(feature = "tokio")]
async fn expiration_task(mut rx: mpsc::UnboundedReceiver<ExpirationCommand>) {
    let mut next_expiration: Option<Instant> = None;

    loop {
        let sleep_duration = if let Some(expires_at) = next_expiration {
            expires_at.saturating_duration_since(Instant::now())
        } else {
            Duration::from_secs(86400)  // Sleep indefinitely
        };

        tokio::select! {
            // Wake up when expiration time is reached
            _ = tokio::time::sleep(sleep_duration) => {
                Self::process_expired_batch();
                next_expiration = Self::get_next_expiration_time();
            }

            // Handle commands from main thread
            Some(cmd) = rx.recv() => {
                match cmd {
                    ExpirationCommand::RescheduleCheck(new_time) => {
                        if next_expiration.map_or(true, |t| new_time < t) {
                            next_expiration = Some(new_time);
                        }
                    }
                    ExpirationCommand::Shutdown => break,
                }
            }
        }
    }
}
```

## Main Synchronization Loop

The main `sync()` function orchestrates the entire process:

```rust
pub fn sync<S, K, T>(
    input_stream: S,
    stream_names: Vec<K>,
    config: Config,
) -> Result<(impl Stream<Item = Result<IndexMap<K, T>, Error>>, impl Stream<Item = Feedback>), Error>
where
    S: Stream<Item = Result<(K, T), Error>>,
    K: Clone + Hash + Eq + Send + 'static,
    T: WithTimestamp + Clone + Send + 'static,
{
    let mut state = State::new(stream_names, config)?;

    input_stream
        .map(move |item| {
            match item {
                Ok((key, message)) => {
                    // 1. Add message to appropriate buffer
                    state.add_message(key, message)?;

                    // 2. Clean up expired messages
                    state.cleanup_expired_messages();

                    // 3. Try to form synchronized groups
                    while let Some(group) = state.try_match() {
                        yield Ok(group);
                    }

                    // 4. Generate feedback
                    let feedback = state.generate_feedback();
                    Ok(feedback)
                }
                Err(e) => Err(e),
            }
        })
}
```

## Performance Characteristics

### Time Complexity

- **Message Addition**: O(1) amortized
- **Window Calculation**: O(S) where S = number of streams
- **Group Formation**: O(S) where S = number of streams
- **Staleness Cleanup**: O(log H) where H = heap size (bounded by config)

### Space Complexity

- **Buffer Storage**: O(B × S) where B = buffer size, S = streams
- **Staleness Heap**: O(H) where H = heap_max_size (typically 256)
- **Timer Wheel**: O(W × M) where W = wheel slots, M = messages per slot

### Configuration Trade-offs

| Configuration    | Precision | Memory Usage | CPU Usage | Use Case                  |
|------------------|-----------|--------------|-----------|---------------------------|
| High Frequency   | 100μs     | Higher       | Higher    | Real-time sensor fusion   |
| Low Frequency    | 1ms       | Medium       | Medium    | Near real-time processing |
| Batch Processing | 10ms      | Lower        | Lower     | Offline data analysis     |

## Edge Cases and Error Handling

### Stream Imbalance

When streams have very different message rates:
- Fast streams accumulate messages waiting for slow streams
- Staleness detection prevents unbounded memory growth
- Configurable timeouts balance latency vs. completeness

### Buffer Overflow

When a stream's buffer reaches capacity:
- Oldest messages are dropped (FIFO policy)
- Feedback mechanism notifies upstream producers
- Graceful degradation maintains system stability

### Clock Skew

When streams have inconsistent timestamps:
- Algorithm is robust to small clock differences
- Large skew may cause messages to be dropped
- Future enhancement: clock skew compensation

### End of Stream

When streams terminate at different times:
- Remaining messages may form partial groups
- Staleness detection eventually cleans up stragglers
- Future enhancement: partial matching strategies

## Implementation Files

The algorithm is distributed across several source files:

- **`src/lib.rs`**: Public API and main `sync()` function
- **`src/state.rs`**: Core synchronization state machine and `try_match()` logic
- **`src/buffer.rs`**: Per-stream message buffering with expiration support
- **`src/staleness.rs`**: Staleness detection with heap and timer wheel
- **`src/config.rs`**: Configuration types and presets

This modular design allows for independent testing and future enhancements to specific components.
