.PHONY: help
help:
	@echo "Available targets:"
	@echo "  build     - Build all targets"
	@echo "  test      - Run all tests"
	@echo "  check     - Quick syntax and type check"
	@echo "  format    - Format code"
	@echo "  lint      - Check formatting and run clippy"
	@echo "  doc       - Generate documentation"
	@echo "  clean     - Clean build artifacts"
	@echo "  all       - Run check, test, and lint"

.PHONY: build
build:
	cargo build --all-targets --all-features

.PHONY: test
test:
	cargo nextest run --no-fail-fast --all-features

.PHONY: check
check:
	cargo check --all-targets

.PHONY: format
format:
	cargo +nightly fmt

.PHONY: lint
lint:
	cargo +nightly fmt --check
	cargo clippy --all-targets --all-features -- -D warnings

.PHONY: doc
doc:
	cargo doc --open

.PHONY: clean
clean:
	cargo clean

.PHONY: all
all: check test lint
