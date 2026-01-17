cmake_minimum_required(VERSION 3.8)
project(conflux_cpp)

# Default to C++17
if(NOT CMAKE_CXX_STANDARD)
  set(CMAKE_CXX_STANDARD 17)
endif()

if(CMAKE_COMPILER_IS_GNUCXX OR CMAKE_CXX_COMPILER_ID MATCHES "Clang")
  add_compile_options(-Wall -Wextra -Wpedantic)
endif()

# Find dependencies
find_package(ament_cmake REQUIRED)
find_package(rclcpp REQUIRED)
find_package(std_msgs REQUIRED)

# Determine Rust build profile
if(CMAKE_BUILD_TYPE STREQUAL "Debug")
  set(CARGO_BUILD_TYPE "debug")
  set(CARGO_BUILD_FLAG "")
else()
  set(CARGO_BUILD_TYPE "release")
  set(CARGO_BUILD_FLAG "--release")
endif()

# Build Rust FFI crate
set(RUST_DIR ${CMAKE_CURRENT_SOURCE_DIR}/rust)
set(RUST_TARGET_DIR ${RUST_DIR}/target)
set(RUST_LIB_PATH ${RUST_TARGET_DIR}/${CARGO_BUILD_TYPE}/libconflux_ffi.so)

# Determine the library extension based on platform
if(APPLE)
  set(RUST_LIB_PATH ${RUST_TARGET_DIR}/${CARGO_BUILD_TYPE}/libconflux_ffi.dylib)
elseif(WIN32)
  set(RUST_LIB_PATH ${RUST_TARGET_DIR}/${CARGO_BUILD_TYPE}/conflux_ffi.dll)
endif()

add_custom_command(
  OUTPUT ${RUST_LIB_PATH}
  COMMAND cargo build ${CARGO_BUILD_FLAG} --manifest-path=${RUST_DIR}/Cargo.toml
  DEPENDS
    ${RUST_DIR}/Cargo.toml
    ${RUST_DIR}/src/lib.rs
    ${RUST_DIR}/cbindgen.toml
    ${RUST_DIR}/build.rs
  WORKING_DIRECTORY ${RUST_DIR}
  COMMENT "Building Rust FFI crate"
)

add_custom_target(conflux_ffi_crate ALL
  DEPENDS ${RUST_LIB_PATH}
)

# Generated header location (created by Rust build.rs)
set(GENERATED_HEADER_DIR ${CMAKE_CURRENT_SOURCE_DIR}/include)

# C++ wrapper library
add_library(${PROJECT_NAME} SHARED
  src/synchronizer.cpp
  src/ffi_bridge.cpp
)

add_dependencies(${PROJECT_NAME} conflux_ffi_crate)

target_include_directories(${PROJECT_NAME} PUBLIC
  $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
  $<BUILD_INTERFACE:${GENERATED_HEADER_DIR}>
  $<INSTALL_INTERFACE:include>
)

target_include_directories(${PROJECT_NAME} PRIVATE
  ${CMAKE_CURRENT_SOURCE_DIR}/src
)

# Link the Rust library
target_link_libraries(${PROJECT_NAME}
  ${RUST_LIB_PATH}
)

ament_target_dependencies(${PROJECT_NAME}
  rclcpp
  std_msgs
)

# Set library properties
set_target_properties(${PROJECT_NAME} PROPERTIES
  VERSION 0.2.0
  SOVERSION 0
)

# Define CONFLUX_BUILDING_DLL for export macros
target_compile_definitions(${PROJECT_NAME} PRIVATE CONFLUX_BUILDING_DLL)

# Install headers
install(
  DIRECTORY include/
  DESTINATION include
)

# Install generated header
install(
  FILES ${GENERATED_HEADER_DIR}/conflux_ffi.h
  DESTINATION include
  OPTIONAL
)

# Install library
install(
  TARGETS ${PROJECT_NAME}
  EXPORT ${PROJECT_NAME}Targets
  LIBRARY DESTINATION lib
  ARCHIVE DESTINATION lib
  RUNTIME DESTINATION bin
  INCLUDES DESTINATION include
)

# Install Rust library
install(
  FILES ${RUST_LIB_PATH}
  DESTINATION lib
)

# Export targets
ament_export_targets(${PROJECT_NAME}Targets HAS_LIBRARY_TARGET)
ament_export_dependencies(rclcpp std_msgs)
ament_export_include_directories(include)

# Testing
if(BUILD_TESTING)
  find_package(ament_lint_auto REQUIRED)
  ament_lint_auto_find_test_dependencies()
endif()

ament_package()
