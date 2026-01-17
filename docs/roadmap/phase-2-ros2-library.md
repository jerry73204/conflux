# Phase 2: Extract conflux-ros2 Library

**Status**: Complete
**Goal**: Extract reusable ROS2 integration utilities into a separate library crate

## Overview

This phase extracts the ROS2-specific utilities from `conflux-node` into a reusable library crate `conflux-ros2`. This enables:

- Other Rust ROS2 nodes to embed synchronization without running a separate node
- Cleaner separation between reusable utilities and node-specific code
- Easier testing of ROS2 integration components

## Target Structure

```
conflux/
├── Cargo.toml
├── crates/
│   ├── conflux-core/             # Pure Rust sync algorithm (unchanged)
│   └── conflux-ros2/             # NEW: ROS2 integration utilities
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs
│           ├── message.rs        # TimestampedMessage, ROS time conversion
│           ├── subscriber.rs     # Dynamic subscription utilities
│           └── qos.rs            # QoS profile helpers
│
└── nodes/
    └── conflux-node/             # Simplified - uses conflux-ros2
        ├── Cargo.toml
        └── src/
            ├── main.rs
            ├── lib.rs
            ├── config.rs         # YAML config (node-specific)
            └── node.rs           # ConfluxNode (node-specific)
```

## What Gets Extracted to conflux-ros2

### From `message.rs`:
- `TimestampedMessage` struct
- `ros_time_to_duration()` function
- `duration_to_ros_time()` function
- `SynchronizedGroup` struct
- `WithTimestamp` implementation for `TimestampedMessage`

### From `subscriber.rs`:
- `DynamicSubscriptionHandle` struct
- `extract_header_stamp()` function
- `create_dynamic_subscription()` function
- `normalize_msg_type()` function

### New in `qos.rs`:
- QoS profile builder helpers (optional)

## What Stays in conflux-node

- `config.rs` - YAML configuration parsing (node-specific format)
- `node.rs` - `ConfluxNode` struct and `run()` logic
- `main.rs` - Entry point

## API Design

### conflux-ros2 Usage Example

```rust
use conflux_ros2::{
    TimestampedMessage, SynchronizedGroup,
    DynamicSubscriptionHandle, create_dynamic_subscription,
    ros_time_to_duration, duration_to_ros_time,
};
use conflux_core::{sync, Config};

// In your own ROS2 node:
let (tx, rx) = mpsc::unbounded_channel();

// Create subscriptions for your topics
let sub = create_dynamic_subscription(
    &node,
    "/camera/image",
    "sensor_msgs/msg/Image",
    qos,
    tx.clone(),
)?;

// Use conflux-core's sync() with the messages
let input_stream = /* stream from rx */;
let (output, feedback) = sync(input_stream, keys, config)?;

// Process synchronized groups
while let Some(group) = output.next().await {
    let synced = SynchronizedGroup::new(timestamp, group);
    // ...
}
```

## Dependencies

### conflux-ros2 Cargo.toml
```toml
[package]
name = "conflux-ros2"

[dependencies]
conflux-core = { workspace = true }
rclrs = { git = "..." }
tokio = { workspace = true }
eyre = { workspace = true }
tracing = { workspace = true }
indexmap = { workspace = true }

# ROS2 message types (wildcard, patched by colcon)
std_msgs = "*"
builtin_interfaces = "*"
```

## Migration Steps

1. Create `crates/conflux-ros2/` directory structure
2. Move `message.rs` content to `conflux-ros2/src/message.rs`
3. Move subscriber utilities to `conflux-ros2/src/subscriber.rs`
4. Create `conflux-ros2/src/lib.rs` with public exports
5. Update `conflux-node` to depend on `conflux-ros2`
6. Update `conflux-node` imports
7. Test build with colcon

## Verification Checklist

- [x] `conflux-ros2` crate created with message.rs and subscriber.rs
- [x] `conflux-node` updated to use `conflux-ros2` dependency
- [x] Unit tests for time conversion included in conflux-ros2
- [x] `conflux-core` tests still pass (57+ tests)
- [ ] Full ROS2 build via colcon (requires ROS2 environment)

## Notes

Like `conflux-node`, the `conflux-ros2` crate uses wildcard versions for ROS2 message
dependencies. It cannot be built with plain `cargo build` - requires colcon to patch
the dependency versions.

## Future Enhancements

- Add `SyncSubscriber` builder API for easier integration
- Add typed message support alongside dynamic messages
- Add QoS preset helpers
