# Phase 3: C++ Bindings (conflux_cpp)

**Status**: Planned
**Goal**: Provide C++ ROS2 developers with a native library for message synchronization

## Overview

Create a C++ ROS package that wraps the Rust synchronization algorithm via FFI. The FFI crate is built internally by CMake, so users don't need separate Rust toolchain setup beyond what colcon-cargo provides.

## Package Structure

```
conflux_cpp/
├── CMakeLists.txt              # ament_cmake, builds FFI + C++ wrapper
├── package.xml
├── include/conflux/
│   ├── synchronizer.hpp        # Main API
│   ├── types.hpp               # SyncGroup, Config types
│   └── visibility.h            # DLL export macros
├── src/
│   ├── synchronizer.cpp        # C++ wrapper implementation
│   └── ffi_bridge.cpp          # Calls into Rust FFI
└── rust/                       # FFI crate (built by CMake)
    ├── Cargo.toml
    ├── cbindgen.toml
    ├── build.rs                # Generates conflux_ffi.h
    └── src/
        └── lib.rs              # #[no_mangle] extern "C" functions
```

## API Design

### C FFI Layer (rust/src/lib.rs)

```rust
use std::ffi::{c_char, c_void};
use std::time::Duration;

/// Opaque handle to synchronizer
pub struct ConfluxSynchronizer { /* ... */ }

#[no_mangle]
pub extern "C" fn conflux_synchronizer_new(
    window_size_ms: u64,
    buffer_size: usize,
) -> *mut ConfluxSynchronizer;

#[no_mangle]
pub extern "C" fn conflux_synchronizer_free(sync: *mut ConfluxSynchronizer);

#[no_mangle]
pub extern "C" fn conflux_add_key(
    sync: *mut ConfluxSynchronizer,
    key: *const c_char,
) -> i32;

#[no_mangle]
pub extern "C" fn conflux_push_message(
    sync: *mut ConfluxSynchronizer,
    key: *const c_char,
    timestamp_ns: i64,
    data: *const c_void,
    data_len: usize,
    user_data: *mut c_void,
) -> i32;

#[no_mangle]
pub extern "C" fn conflux_poll(
    sync: *mut ConfluxSynchronizer,
    callback: extern "C" fn(*const ConfluxSyncGroup, *mut c_void),
    user_data: *mut c_void,
) -> i32;
```

### C++ Wrapper (include/conflux/synchronizer.hpp)

```cpp
#pragma once

#include <chrono>
#include <functional>
#include <memory>
#include <string>
#include <unordered_map>
#include <vector>

#include "rclcpp/rclcpp.hpp"
#include "conflux/types.hpp"

namespace conflux {

struct Config {
    std::chrono::milliseconds window_size{50};
    size_t buffer_size{64};
};

class Synchronizer {
public:
    explicit Synchronizer(const Config& config);
    ~Synchronizer();

    // Non-copyable
    Synchronizer(const Synchronizer&) = delete;
    Synchronizer& operator=(const Synchronizer&) = delete;

    // Add a topic to synchronize
    template<typename MsgT>
    void add_subscription(
        rclcpp::Node::SharedPtr node,
        const std::string& topic,
        const rclcpp::QoS& qos = rclcpp::SensorDataQoS()
    );

    // Set callback for synchronized groups
    void on_synchronized(std::function<void(const SyncGroup&)> callback);

    // Process pending messages (call from timer or spin)
    void spin_once();

private:
    class Impl;
    std::unique_ptr<Impl> impl_;
};

}  // namespace conflux
```

### Usage Example

```cpp
#include <rclcpp/rclcpp.hpp>
#include <sensor_msgs/msg/image.hpp>
#include <sensor_msgs/msg/point_cloud2.hpp>
#include <conflux/synchronizer.hpp>

class MySyncNode : public rclcpp::Node {
public:
    MySyncNode() : Node("my_sync_node") {
        conflux::Config config;
        config.window_size = std::chrono::milliseconds(50);
        config.buffer_size = 64;

        sync_ = std::make_unique<conflux::Synchronizer>(config);

        sync_->add_subscription<sensor_msgs::msg::Image>(
            shared_from_this(), "/camera/image");
        sync_->add_subscription<sensor_msgs::msg::PointCloud2>(
            shared_from_this(), "/lidar/points");

        sync_->on_synchronized([this](const conflux::SyncGroup& group) {
            auto image = group.get<sensor_msgs::msg::Image>("/camera/image");
            auto points = group.get<sensor_msgs::msg::PointCloud2>("/lidar/points");
            process(image, points);
        });

        timer_ = create_wall_timer(
            std::chrono::milliseconds(10),
            [this]() { sync_->spin_once(); }
        );
    }

private:
    std::unique_ptr<conflux::Synchronizer> sync_;
    rclcpp::TimerBase::SharedPtr timer_;
};
```

## CMakeLists.txt

```cmake
cmake_minimum_required(VERSION 3.8)
project(conflux_cpp)

find_package(ament_cmake REQUIRED)
find_package(rclcpp REQUIRED)
find_package(std_msgs REQUIRED)

# Build Rust FFI crate
include(ExternalProject)
ExternalProject_Add(conflux_ffi_crate
    SOURCE_DIR ${CMAKE_CURRENT_SOURCE_DIR}/rust
    CONFIGURE_COMMAND ""
    BUILD_COMMAND cargo build --release --manifest-path=${CMAKE_CURRENT_SOURCE_DIR}/rust/Cargo.toml
    BUILD_IN_SOURCE TRUE
    INSTALL_COMMAND ""
    BUILD_BYPRODUCTS ${CMAKE_CURRENT_SOURCE_DIR}/rust/target/release/libconflux_ffi.so
)

# Generate C header with cbindgen
add_custom_command(
    OUTPUT ${CMAKE_CURRENT_BINARY_DIR}/include/conflux_ffi.h
    COMMAND cbindgen --config ${CMAKE_CURRENT_SOURCE_DIR}/rust/cbindgen.toml
            --crate conflux-ffi
            --output ${CMAKE_CURRENT_BINARY_DIR}/include/conflux_ffi.h
            ${CMAKE_CURRENT_SOURCE_DIR}/rust
    DEPENDS conflux_ffi_crate
)

# C++ wrapper library
add_library(${PROJECT_NAME} SHARED
    src/synchronizer.cpp
    src/ffi_bridge.cpp
)
add_dependencies(${PROJECT_NAME} conflux_ffi_crate)

target_include_directories(${PROJECT_NAME} PUBLIC
    $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
    $<BUILD_INTERFACE:${CMAKE_CURRENT_BINARY_DIR}/include>
    $<INSTALL_INTERFACE:include>
)

target_link_libraries(${PROJECT_NAME}
    ${CMAKE_CURRENT_SOURCE_DIR}/rust/target/release/libconflux_ffi.so
)

ament_target_dependencies(${PROJECT_NAME}
    rclcpp
    std_msgs
)

# Install
install(
    DIRECTORY include/
    DESTINATION include
)
install(
    TARGETS ${PROJECT_NAME}
    EXPORT ${PROJECT_NAME}Targets
    LIBRARY DESTINATION lib
    ARCHIVE DESTINATION lib
    RUNTIME DESTINATION bin
)
install(
    FILES ${CMAKE_CURRENT_SOURCE_DIR}/rust/target/release/libconflux_ffi.so
    DESTINATION lib
)

ament_export_targets(${PROJECT_NAME}Targets HAS_LIBRARY_TARGET)
ament_export_dependencies(rclcpp std_msgs)

ament_package()
```

## package.xml

```xml
<?xml version="1.0"?>
<package format="3">
  <name>conflux_cpp</name>
  <version>0.2.0</version>
  <description>C++ library for multi-stream message synchronization</description>

  <maintainer email="todo@example.com">TODO</maintainer>
  <license>MIT</license>
  <license>Apache-2.0</license>

  <buildtool_depend>ament_cmake</buildtool_depend>

  <depend>rclcpp</depend>
  <depend>std_msgs</depend>

  <test_depend>ament_lint_auto</test_depend>

  <export>
    <build_type>ament_cmake</build_type>
  </export>
</package>
```

## Implementation Steps

1. Create `conflux_cpp/` directory structure
2. Implement FFI crate (`rust/src/lib.rs`)
3. Configure cbindgen for header generation
4. Implement C++ wrapper classes
5. Set up CMakeLists.txt with Cargo integration
6. Add usage examples
7. Test with sample C++ node

## Verification Checklist

- [ ] `colcon build --packages-select conflux_cpp` succeeds
- [ ] FFI shared library is built and installed
- [ ] C header is generated correctly
- [ ] C++ wrapper compiles and links
- [ ] Example node synchronizes messages correctly
- [ ] Library can be found via `find_package(conflux_cpp)`

## Dependencies

- Rust toolchain (for building FFI crate)
- cbindgen (for generating C header)
- rclcpp (ROS2 C++ client library)
