# Contributing to conflux-core

Thank you for your interest in contributing to the conflux-core! This document provides comprehensive guidelines for developers who want to contribute to the project.

## Getting Started

### Prerequisites

- Rust 1.70+ (stable toolchain)
- Cargo and rustup
- Git

### Development Setup

```bash
# Clone the repository
git clone https://github.com/your-org/multi-stream-synchronizer.git
cd multi-stream-synchronizer

# Run tests
cargo test

# Run with tokio features (for staleness tests)
cargo test --features tokio

# Check formatting and linting
make lint
```

## Project Architecture

For a detailed understanding of the synchronization algorithm, see [ALGORITHM.md](ALGORITHM.md).

### Project Structure

```
src/
├── lib.rs              # Public API and main sync function
├── buffer.rs           # Stream buffer management
├── state.rs            # Synchronization state machine
├── staleness.rs        # Message staleness detection
└── config.rs           # Configuration types

tests/
├── basic_tests.rs      # Basic synchronization tests
├── staleness_basic_tests.rs    # Staleness feature tests
└── staleness_tokio_tests.rs    # Tokio-specific staleness tests

docs/
├── ALGORITHM.md        # Technical algorithm documentation
├── IMPROVE.md          # Planned improvements and roadmap
└── CONTRIBUTING.md     # This document
```

### Key Components

1. **`src/lib.rs`**: Public API surface and main `sync()` function
2. **`src/state.rs`**: Core synchronization logic and state machine
3. **`src/buffer.rs`**: Per-stream message buffering with FIFO semantics
4. **`src/staleness.rs`**: Advanced staleness detection system
5. **`src/config.rs`**: Configuration types and preset builders

## Development Guidelines

### Code Style

- Follow standard Rust naming conventions and idioms
- Use `rustfmt` for consistent formatting (enforced by CI)
- Prefer explicit types over inference when it improves readability
- Add comprehensive documentation for public APIs
- Keep performance in mind - this library targets real-time applications

### Testing Strategy

The project maintains comprehensive test coverage across multiple categories:

#### Unit Tests
Test individual components in isolation:
```bash
# Test specific modules
cargo test buffer::tests
cargo test state::tests
cargo test staleness::tests
```

#### Integration Tests
Test end-to-end synchronization scenarios:
```bash
# Basic synchronization functionality
cargo test basic_tests

# Staleness detection (requires tokio feature)
cargo test staleness_tests --features tokio
```

#### Stress Tests
High-volume, high-frequency message processing:
```bash
# Run stress tests (may take longer)
cargo test stress --features tokio -- --nocapture
```

### Performance Testing

```bash
# Run performance benchmarks (requires nightly)
cargo +nightly bench

# Profile memory usage
cargo test --features tokio -- --nocapture | grep -E "(memory|heap)"

# Check for memory leaks in long-running tests
cargo test staleness_stress --features tokio -- --nocapture
```

### Linting and Formatting

Always run linting before submitting changes:

```bash
# Check formatting (CI enforced)
make lint

# Or manually:
cargo +nightly fmt --check
cargo clippy --all-targets --all-features -- -D warnings
```

## Contributing Workflow

### 1. Issue Discussion

Before starting work on a feature or fix:
- Check existing issues and PRs
- Open an issue to discuss your proposed changes
- Get feedback from maintainers before implementation

### 2. Branch and Development

```bash
# Create a feature branch
git checkout -b feature/your-feature-name

# Make your changes with comprehensive tests
# Run tests frequently during development
cargo test --features tokio

# Ensure all linting passes
make lint
```

### 3. Testing Requirements

All contributions must include:
- **Unit tests** for new functionality
- **Integration tests** for end-to-end scenarios
- **Documentation** for public APIs
- **Performance validation** for critical paths

#### Example Test Structure

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_functionality() {
        // Test basic happy path
    }

    #[test]
    fn test_edge_cases() {
        // Test boundary conditions
    }

    #[tokio::test]
    async fn test_async_behavior() {
        // Test async/await functionality
    }
}
```

### 4. Documentation

Update documentation for any public API changes:
- Add rustdoc comments for new public functions/types
- Update examples in README.md if API changes
- Add entries to IMPROVE.md for significant enhancements
- Update ALGORITHM.md for algorithmic changes

### 5. Pull Request

When submitting a PR:
- Write a clear description of your changes
- Reference any related issues
- Include performance impact if relevant
- Ensure all CI checks pass

## Feature Development Areas

See [IMPROVE.md](IMPROVE.md) for detailed improvement opportunities. Key areas include:

### High Priority
- **Enhanced Matching Strategies**: Partial matching, priority-based selection
- **Window Boundary Clarification**: Better boundary semantics with `std::ops::Bound`
- **Buffer Management**: Advanced eviction policies and adaptive sizing

### Medium Priority
- **Clock Skew Handling**: Compensation for timing irregularities
- **Feedback Enhancement**: Better upstream signaling
- **Algorithm Optimizations**: Performance improvements for high-throughput scenarios

### Performance Focus Areas

This library targets real-time applications, so performance is critical:

1. **Low Latency**: Minimize processing delays
2. **High Throughput**: Support thousands of messages per second
3. **Memory Efficiency**: Bounded memory usage even with stream imbalances
4. **CPU Efficiency**: Minimize overhead in hot paths

## Testing Guidelines

### Test Categories

1. **Correctness Tests**: Verify algorithm produces correct synchronization
2. **Performance Tests**: Validate throughput and latency requirements
3. **Stress Tests**: High-volume scenarios and edge cases
4. **Memory Tests**: Ensure bounded memory usage
5. **Integration Tests**: End-to-end scenarios with real data patterns

### Writing Effective Tests

```rust
// Good: Clear test name and purpose
#[test]
fn test_window_boundary_with_exact_timestamp_match() {
    // Arrange: Set up test scenario
    let config = Config::basic(Duration::from_millis(100), None, 32);

    // Act: Perform the operation
    let result = sync_messages(messages, config);

    // Assert: Verify expected behavior
    assert_eq!(result.len(), expected_groups);
}

// Good: Test edge cases
#[test]
fn test_buffer_overflow_with_slow_stream() {
    // Test what happens when one stream is much slower
}

// Good: Performance validation
#[test]
fn test_high_frequency_throughput() {
    let start = Instant::now();
    // Process 10000 messages
    let duration = start.elapsed();
    assert!(duration < Duration::from_millis(100), "Too slow: {:?}", duration);
}
```

## Debugging and Profiling

### Debug Builds

```bash
# Run with debug output
RUST_LOG=debug cargo test -- --nocapture

# Enable tracing for specific modules
RUST_LOG=conflux_core::staleness=trace cargo test
```

### Performance Profiling

```bash
# CPU profiling with perf (Linux)
perf record --call-graph=dwarf cargo test --release
perf report

# Memory profiling with valgrind
valgrind --tool=massif cargo test --release
```

## Common Issues and Solutions

### Build Issues

```bash
# Clean rebuild
cargo clean && cargo build

# Check for feature conflicts
cargo check --all-features
cargo check --no-default-features
```

### Test Failures

```bash
# Run specific failing test with output
cargo test failing_test_name -- --nocapture

# Run with backtrace
RUST_BACKTRACE=1 cargo test
```

### Performance Regressions

```bash
# Compare before/after with benchmarks
cargo bench > before.txt
# Make changes
cargo bench > after.txt
# Compare results
```

## Release Process

For maintainers preparing releases:

1. **Version Bump**: Update version in `Cargo.toml`
2. **Changelog**: Update with new features and fixes
3. **Documentation**: Ensure all docs are current
4. **Testing**: Full test suite on multiple platforms
5. **Benchmarks**: Verify no performance regressions

## Getting Help

- **Issues**: GitHub issues for bugs and feature requests
- **Discussions**: GitHub discussions for questions and ideas
- **Documentation**: See ALGORITHM.md for technical details
- **Code Review**: Maintainers provide feedback on PRs

## Code of Conduct

We are committed to providing a welcoming and inclusive environment. Please:
- Be respectful and constructive in discussions
- Focus on technical merit in code reviews
- Help newcomers learn the codebase
- Report any inappropriate behavior to maintainers

Thank you for contributing to conflux-core!