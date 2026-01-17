# Conflux Distribution Roadmap

## Overview

Conflux is distributed as a git submodule that users add to their ROS2 workspace. This enables:

1. **Standalone node** - Run a synchronization node via launch files
2. **Library integration** - Embed synchronization in Rust/C++/Python nodes

## Target Structure

```
conflux/                          # Users add as git submodule
├── crates/
│   ├── conflux-core/             # Pure Rust sync algorithm
│   └── conflux-ros2/             # Rust ROS2 integration library
│
├── conflux_cpp/                  # C++ ROS package (includes FFI)
│   ├── CMakeLists.txt
│   ├── package.xml
│   ├── include/
│   │   ├── conflux_ffi.h         # Generated C header
│   │   └── conflux/
│   │       ├── synchronizer.hpp
│   │       ├── types.hpp
│   │       └── visibility.h
│   ├── src/
│   │   ├── synchronizer.cpp
│   │   ├── ffi_bridge.cpp
│   │   └── ffi_bridge.hpp
│   ├── examples/
│   │   └── sync_node.cpp
│   └── rust/                     # FFI crate (built by CMake)
│       ├── Cargo.toml
│       ├── cbindgen.toml
│       ├── build.rs
│       └── src/lib.rs
│
├── conflux_py/                   # Python ROS package
│   ├── package.xml
│   ├── setup.py
│   ├── conflux_py/
│   │   ├── __init__.py
│   │   └── synchronizer.py
│   └── rust/                     # PyO3 crate (built by setup.py)
│       ├── Cargo.toml
│       └── src/lib.rs
│
├── conflux_node/                 # Standalone ROS2 node
│   ├── Cargo.toml
│   ├── package.xml
│   ├── config/
│   ├── launch/
│   └── src/
│
├── docs/
│   └── roadmap/
│
├── Cargo.toml                    # Workspace for pure Rust crates
└── README.md
```

## User Workflow

```bash
# Add conflux to ROS2 workspace
cd ~/ros2_ws/src
git submodule add https://github.com/jerry73204/conflux.git

# Build all packages (or select specific ones)
cd ~/ros2_ws
colcon build                                    # Build everything
colcon build --packages-select conflux_cpp      # C++ library only
colcon build --packages-select conflux_py       # Python library only
colcon build --packages-select conflux          # Standalone node only
```

## Package Summary

| Package | Language | Type | Use Case |
|---------|----------|------|----------|
| `conflux-core` | Rust | Library | Core algorithm, no ROS deps |
| `conflux-ros2` | Rust | ROS Library | Rust ROS2 developers |
| `conflux_cpp` | C++ | ROS Library | C++ ROS2 developers |
| `conflux_py` | Python | ROS Library | Python ROS2 developers |
| `conflux` | Rust | ROS Node | Standalone synchronization node |

## Build Dependency Graph

```
conflux-core (Rust, pure)
       │
       ├──────────────────┬────────────────────┐
       ▼                  ▼                    ▼
conflux-ros2         conflux_cpp/rust     conflux_py/rust
(Rust ROS2)          (FFI crate)          (PyO3 crate)
       │                  │                    │
       ▼                  ▼                    ▼
conflux_node         conflux_cpp          conflux_py
(Standalone)         (C++ wrapper)        (Python wrapper)
```

## Phases

| Phase | Description | Status |
|-------|-------------|--------|
| 1 | Workspace setup, conflux-core extraction | ✅ Complete |
| 2 | conflux-ros2 library extraction | ✅ Complete |
| 3 | C++ bindings (conflux_cpp) | ✅ Complete |
| 4 | Python bindings (conflux_py) | ✅ Complete |
| 5 | Flatten directory structure | 📋 Planned |

## Phase Details

- [Phase 1: Workspace Setup](phase-1-workspace-setup.md)
- [Phase 2: ROS2 Library](phase-2-ros2-library.md)
- [Phase 3: C++ Bindings](phase-3-cpp-bindings.md)
- [Phase 4: Python Bindings](phase-4-python-bindings.md)
- [Phase 5: Structure Cleanup](phase-5-structure-cleanup.md)
