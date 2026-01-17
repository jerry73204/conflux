# Phase 1: Workspace Monorepo Setup

**Status**: Complete
**Goal**: Restructure the project as a Cargo workspace to support multiple distribution targets

## Overview

This phase converts the single-crate conflux project into a Cargo workspace monorepo. This enables:

- Independent versioning of core algorithm vs ROS2 integration
- Future addition of C/C++ bindings (`conflux-ffi`)
- Future addition of Python bindings (`conflux-py`)
- Clear separation between the synchronization library and ROS2-specific code

## Final Structure

```
conflux/
├── Cargo.toml                    # Workspace root
├── README.md
├── LICENSE-MIT
├── LICENSE-APACHE
├── justfile
├── .envrc
├── CLAUDE.md
│
├── crates/
│   └── conflux-core/             # Core sync algorithm (renamed from multi-stream-synchronizer)
│       ├── Cargo.toml
│       ├── README.md
│       ├── ALGORITHM.md
│       ├── CONTRIBUTING.md
│       └── src/
│           ├── lib.rs
│           ├── buffer.rs
│           ├── config.rs
│           ├── staleness.rs
│           ├── state.rs
│           ├── sync.rs
│           └── types.rs
│
├── nodes/
│   └── conflux-node/             # ROS2 standalone node
│       ├── Cargo.toml
│       ├── package.xml           # ROS2 package manifest
│       └── src/
│           ├── main.rs
│           ├── lib.rs
│           ├── config.rs
│           ├── message.rs
│           ├── node.rs
│           └── subscriber.rs
│
├── config/                       # Unchanged
├── launch/                       # Unchanged
├── test/                         # Unchanged
└── docs/
    └── roadmap/
```

## Changes Made

### Step 1: Create Workspace Root Cargo.toml

Created workspace `Cargo.toml` at repository root with:
- Workspace members: `crates/conflux-core` (ROS2 node excluded due to wildcard deps)
- Shared workspace dependencies for consistency
- `test-release` profile for optimized debug builds

### Step 2: Move Core Library

1. Removed `multi-stream-synchronizer` git submodule
2. Copied contents to `crates/conflux-core/`
3. Updated `Cargo.toml`:
   - Renamed package to `conflux-core`
   - Updated to edition 2024
   - Inherited workspace fields
4. Updated all internal references from `multi_stream_synchronizer` to `conflux_core`

### Step 3: Move ROS2 Node

1. Created `nodes/conflux-node/` directory
2. Moved `src/` contents and `package.xml`
3. Created new `Cargo.toml` that depends on `conflux-core`
4. Updated imports from `multi_stream_synchronizer` to `conflux_core`

### Step 4: Update Build System

Updated `justfile` with:
- `build-core`: Build pure Rust library without ROS2
- `build`: Build ROS2 node via colcon
- `test-core`: Test core library independently
- `lint-core`: Lint core library independently
- Backwards-compatible `test-lib` alias

### Step 5: Update Documentation

- Updated `CLAUDE.md` with new project structure
- Documented architecture separation (core vs node)
- Updated all command references

## Verification Checklist

- [x] `cargo build -p conflux-core` succeeds
- [x] `cargo test -p conflux-core --features tokio` passes (all tests pass)
- [ ] `just build` builds the ROS2 node via colcon (requires ROS2 environment)
- [ ] `ros2 launch conflux conflux.launch.xml` works (requires ROS2 environment)
- [x] Core library tests pass independently

## Breaking Changes

- **Crate rename**: `multi-stream-synchronizer` → `conflux-core`
  - Users importing the library must update their dependencies
  - Import path changes from `multi_stream_synchronizer::*` to `conflux_core::*`

- **Directory structure**: Source files moved to new locations
  - Core library: `crates/conflux-core/`
  - ROS2 node: `nodes/conflux-node/`

- **Submodule removed**: `multi-stream-synchronizer` is no longer a git submodule
  - Code is now directly part of the repository

## Notes

The ROS2 node (`nodes/conflux-node`) is excluded from the workspace `members` because it uses
wildcard version dependencies (`*`) for ROS2 message crates. These get patched at build time
by `colcon-cargo-ros2` which generates `build/ros2_cargo_config.toml`. This means:

- `cargo build` works for `conflux-core` without ROS2
- `just build` (colcon) is required for the full ROS2 node

## Future Phases

After this phase is complete:

- **Phase 2**: Extract `conflux-ros2` library (reusable ROS2 integration utilities)
- **Phase 3**: Add `conflux-ffi` crate (C/C++ bindings)
- **Phase 4**: Add `conflux-py` crate (Python bindings via PyO3)
