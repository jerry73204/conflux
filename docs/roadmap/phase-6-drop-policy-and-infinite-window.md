# Phase 6: Drop Policy and Infinite Window Support

## Overview

Add configurable message drop policies and infinite window support to enable proper offline (rosbag) and realtime sensor processing.

## Current Limitations

1. **No buffer capacity enforcement**: `try_push()` only rejects out-of-order messages, not capacity overflow
2. **Window always drops**: `try_match()` unconditionally drops messages outside the window
3. **No drop policy**: Users cannot choose behavior when buffer is full

## Design

### Drop Policy

Two policies for buffer overflow:

| Policy | Behavior | Use Case |
|--------|----------|----------|
| **RejectNew** | Return error immediately | Offline/rosbag, caller handles overflow |
| **DropOldest** | Evict oldest, accept new | Realtime, always latest data |

### Infinite Window

Make `window_size` optional. When `None` (or `0` in FFI), disable time-based message dropping in `try_match()`.

### Configuration Presets

| Mode | Window | Buffer | Policy |
|------|--------|--------|--------|
| Offline | Infinite | Large (100+) | RejectNew |
| Realtime | 50ms | Small (2-5) | DropOldest |

## Implementation

1. **Core**: Add `DropPolicy` enum, update `State::push()` and `try_match()`
2. **FFI**: Add C-compatible enum, update config struct
3. **Python**: Add enum and `SyncConfig` factory methods (`offline()`, `realtime()`)
4. **ROS2**: Add `sync_drop_policy` parameter to solver nodes and launch files

## Breaking Changes

- `window_size` type: `Duration` → `Option<Duration>`
- `push()` may return `BufferFull` error
