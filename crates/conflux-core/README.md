# conflux-core

A Rust library for synchronizing timestamped messages from multiple data streams using time-window based grouping. This library is designed for real-time applications that need to correlate events across different data sources with precise timing control.

The conflux-core solves the common problem of correlating timestamped data from multiple sources. It groups messages that arrive within configurable time windows, ensuring that each group contains at most one message from each stream. This is particularly useful for:

### Key Features

- **Configurable Time Windows**: Define precise synchronization windows for your use case
- **Message Staleness Prevention**: Automatic expiration of old messages to prevent memory buildup
- **High-Performance**: Optimized for low-latency real-time processing
- **Flexible Configuration**: Support for both immediate and lazy message expiration
- **Async Support**: Built with async/await support using Tokio (optional)

## Quick Start

Add this to your `Cargo.toml`:

```toml
[dependencies]
multi-stream-synchronizer = "0.1.0"

# For immediate message expiration (recommended for real-time applications)
multi-stream-synchronizer = { version = "0.1.0", features = ["tokio"] }
```

### Basic Example

```rust
use conflux_core::{sync, Config, WithTimestamp};
use futures::stream::{self, TryStreamExt};
use std::time::Duration;

#[derive(Debug, Clone)]
struct SensorReading {
    timestamp: Duration,
    value: f64,
    sensor_id: String,
}

impl WithTimestamp for SensorReading {
    fn timestamp(&self) -> Duration {
        self.timestamp
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create sample data from two sensors
    let messages = vec![
        ("gps", SensorReading { timestamp: Duration::from_millis(1000), value: 42.3, sensor_id: "gps".into() }),
        ("imu", SensorReading { timestamp: Duration::from_millis(1010), value: 9.8, sensor_id: "imu".into() }),
        ("gps", SensorReading { timestamp: Duration::from_millis(2000), value: 42.4, sensor_id: "gps".into() }),
        ("imu", SensorReading { timestamp: Duration::from_millis(2005), value: 9.9, sensor_id: "imu".into() }),
    ];

    // Configure synchronization with 50ms time windows
    let config = Config::basic(
        Duration::from_millis(50),  // Time window size
        None,                       // No fixed start time
        64                          // Buffer size per stream
    );

    // Create input stream
    let input_stream = stream::iter(messages.into_iter().map(Ok));

    // Start synchronization
    let (output_stream, _feedback) = sync(input_stream, vec!["gps", "imu"], config)?;

    // Process synchronized groups
    let groups: Vec<_> = output_stream.try_collect().await?;

    for group in groups {
        println!("Synchronized group:");
        for (stream_name, reading) in group {
            println!("  {}: {} at {:?}", stream_name, reading.value, reading.timestamp);
        }
    }

    Ok(())
}
```

### With Staleness Prevention

For real-time applications, enable staleness prevention to ensure messages don't accumulate indefinitely:

```rust
use conflux_core::{sync, Config, StalenessConfig, WithTimestamp};

// Configure staleness detection for real-time processing
let staleness_config = StalenessConfig::high_frequency(); // Sub-millisecond precision

let config = Config::with_staleness(
    Duration::from_millis(50),    // Window size
    None,                         // No fixed start time
    64,                           // Buffer size
    staleness_config
);

// Messages will automatically expire if they can't be synchronized within their time limits
```

## Configuration

### Basic Configuration

```rust
use conflux_core::Config;
use std::time::Duration;

// Simple configuration
let config = Config::basic(
    Duration::from_millis(100),  // Time window: 100ms
    None,                        // Start immediately
    32                           // Buffer 32 messages per stream
);

// With fixed start time
let config = Config::basic(
    Duration::from_millis(100),
    Some(Duration::from_secs(10)), // Start at 10 seconds
    32
);
```

### Staleness Configuration

```rust
use conflux_core::StalenessConfig;

// Predefined configurations for common use cases
let high_freq = StalenessConfig::high_frequency();    // 100μs precision, real-time
let low_latency = StalenessConfig::low_frequency();   // 1ms precision, near real-time
let batch = StalenessConfig::batch_processing();      // 10ms precision, lazy checking

// Custom configuration
let custom = StalenessConfig {
    heap_max_size: 256,                              // Max 256 messages in priority queue
    heap_time_horizon: Duration::from_millis(100),   // 100ms lookahead window
    precision_gap: Duration::from_micros(500),       // 500μs precision
    timer_wheel_slots: 128,                          // Timer wheel size
    enable_immediate_expiration: true,               // Requires tokio feature
};
```

## How the Algorithm Works

The conflux-core uses time-window based grouping to correlate messages from multiple streams. For a detailed explanation of the algorithm internals, see [ALGORITHM.md](ALGORITHM.md).

### Quick Overview

1. **Time Windows**: Messages are grouped within configurable time windows
2. **Stream Buffers**: Each stream maintains a buffer of incoming messages
3. **Synchronization**: The algorithm matches one message per stream within each window
4. **Staleness Prevention**: Old messages are automatically expired to prevent memory buildup

```
Timeline:    [----Window 1----][----Window 2----][----Window 3----]
Stream A:      A1                 A2                 A3
Stream B:        B1                 B2                 B3
Stream C:          C1                 C2                 C3
Groups:      {A1,B1,C1}         {A2,B2,C2}         {A3,B3,C3}
```

## API Reference

### Core Functions

```rust
// Main synchronization function
pub fn sync<S, K, T>(
    input_stream: S,
    stream_names: Vec<K>,
    config: Config,
) -> Result<(impl Stream<Item = Result<IndexMap<K, T>, Error>>, impl Stream<Item = Feedback>), Error>

// Configuration constructors
impl Config {
    pub fn basic(window_size: Duration, start_time: Option<Duration>, buf_size: usize) -> Self
    pub fn with_staleness(window_size: Duration, start_time: Option<Duration>, buf_size: usize, staleness_config: StalenessConfig) -> Self
}

// Staleness configurations
impl StalenessConfig {
    pub fn high_frequency() -> Self      // Real-time, sub-millisecond precision
    pub fn low_frequency() -> Self       // Near real-time, millisecond precision
    pub fn batch_processing() -> Self    // Batch processing, lazy checking
}
```

### Traits

```rust
// Messages must implement this trait to provide timestamps
pub trait WithTimestamp {
    fn timestamp(&self) -> Duration;
}
```

## Contributing

We welcome contributions!

- **For Users**: Found a bug or have a feature request? Please [open an issue](https://github.com/your-org/multi-stream-synchronizer/issues)
- **For Developers**: See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, testing guidelines, and contribution workflow
- **Algorithm Details**: See [ALGORITHM.md](ALGORITHM.md) for technical implementation details

## Documentation

- **[ALGORITHM.md](ALGORITHM.md)**: Detailed technical explanation of the synchronization algorithm
- **[CONTRIBUTING.md](CONTRIBUTING.md)**: Development setup, testing guidelines, and contribution workflow

## License

Licensed under either of:
- Apache License, Version 2.0
- MIT License

at your option.
