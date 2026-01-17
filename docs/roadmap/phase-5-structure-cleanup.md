# Phase 5: Structure Cleanup

**Status**: Completed
**Goal**: Flatten directory structure for simpler navigation and package management

## Overview

Reorganize the repository to have a flatter structure where each ROS package is at the top level (except for pure Rust crates which stay in `crates/`).

## Current vs Target Structure

### Current Structure
```
conflux/
├── crates/
│   ├── conflux-core/
│   └── conflux-ros2/
├── nodes/
│   └── conflux-node/          # Nested under nodes/
└── docs/
```

### Target Structure
```
conflux/
├── crates/
│   ├── conflux-core/          # Pure Rust (unchanged)
│   └── conflux-ros2/          # Rust ROS2 library (unchanged)
├── conflux_node/              # Flattened (was nodes/conflux-node)
├── conflux_cpp/               # New (Phase 3)
├── conflux_py/                # New (Phase 4)
└── docs/
```

## Changes Required

### 1. Move conflux-node to top level

```bash
# Move and rename
mv nodes/conflux-node conflux_node
rmdir nodes

# Update internal references
# - Cargo.toml paths
# - package.xml if needed
```

### 2. Update Workspace Cargo.toml

```toml
[workspace]
resolver = "2"
members = [
    "crates/conflux-core",
]
exclude = [
    "crates/conflux-ros2",
    "conflux_node",      # Changed from nodes/conflux-node
    "conflux_cpp",
    "conflux_py",
]
```

### 3. Update CLAUDE.md Project Structure

Document the new flattened structure.

### 4. Update CI/CD (if applicable)

Update any build scripts or CI configuration that reference the old paths.

## Migration Steps

1. Move `nodes/conflux-node/` to `conflux_node/`
2. Remove empty `nodes/` directory
3. Update root `Cargo.toml` exclude paths
4. Update `conflux_node/Cargo.toml` dependency paths:
   - `conflux-ros2 = { path = "crates/conflux-ros2" }` → `{ path = "../crates/conflux-ros2" }`
   Wait, that's wrong. Let me reconsider.

Actually with the flattened structure:
- `conflux_node/` is at root level
- `crates/conflux-ros2/` is at root level under crates/

So the path from `conflux_node/Cargo.toml` to `conflux-ros2` would be:
- `path = "../crates/conflux-ros2"` - No, this would go up to parent of conflux repo
- Actually `conflux_node` at root means the path should be `path = "crates/conflux-ros2"` from the repo root perspective

Wait, Cargo.toml paths are relative to the Cargo.toml file location. So from `conflux_node/Cargo.toml`:
- To reach `crates/conflux-ros2/`, the path is `../crates/conflux-ros2`

Let me recalculate:
- `conflux_node/Cargo.toml` is at `/repo/conflux_node/Cargo.toml`
- `conflux-ros2` is at `/repo/crates/conflux-ros2/`
- Relative path: `../crates/conflux-ros2`

Currently it's:
- `nodes/conflux-node/Cargo.toml` at `/repo/nodes/conflux-node/Cargo.toml`
- Relative path: `../../crates/conflux-ros2`

So after flattening, paths change from `../../crates/` to `../crates/`.

4. Update `conflux_node/Cargo.toml` dependency paths
5. Update documentation
6. Verify build still works

## Verification Checklist

- [x] `just build` succeeds
- [x] `just test-core` succeeds
- [x] All packages discoverable by colcon
- [x] Documentation updated (CLAUDE.md)

## Notes

This phase should be done after Phase 3 and 4 are complete, as it's easier to set up the new packages in the final location from the start.

Alternatively, this phase can be done earlier (before Phase 3/4) so that the new packages are created in their final locations.

## Recommendation

**Do Phase 5 before Phase 3 and 4** to avoid having to move packages after they're created. The updated order would be:

1. Phase 1: Workspace setup ✅
2. Phase 2: conflux-ros2 extraction ✅
3. **Phase 5: Structure cleanup** (move conflux-node to top level)
4. Phase 3: C++ bindings (create at top level)
5. Phase 4: Python bindings (create at top level)
