# Phase 4: Python Bindings (conflux_py)

**Status**: Completed
**Goal**: Provide Python ROS2 developers with a native library for message synchronization

## Overview

Create a Python ROS package that wraps the Rust synchronization algorithm via PyO3. The native module is built using maturin, providing a high-performance Python extension.

## Package Structure

```
conflux_py/
├── package.xml
├── pyproject.toml              # maturin build configuration
├── setup.py                    # ament_python compatibility
├── setup.cfg
├── README.md
├── resource/conflux_py         # ament resource marker
├── conflux_py/
│   ├── __init__.py
│   ├── synchronizer.py         # High-level ROS2 Python API
│   └── _conflux_py.pyi         # Type stubs for native module
├── rust/                       # PyO3 crate
│   ├── Cargo.toml
│   └── src/
│       └── lib.rs              # #[pymodule] definitions
├── examples/
│   └── sync_node.py            # Example ROS2 node
└── test/
    └── test_synchronizer.py
```

## API Design

### PyO3 Native Module (rust/src/lib.rs)

```rust
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict};
use std::collections::HashMap;
use std::time::Duration;

#[pyclass]
struct SyncConfig {
    #[pyo3(get, set)]
    window_size_ms: u64,
    #[pyo3(get, set)]
    buffer_size: usize,
}

#[pymethods]
impl SyncConfig {
    #[new]
    #[pyo3(signature = (window_size_ms=50, buffer_size=64))]
    fn new(window_size_ms: u64, buffer_size: usize) -> Self {
        Self { window_size_ms, buffer_size }
    }
}

#[pyclass]
struct SyncGroup {
    timestamp_ns: i64,
    messages: HashMap<String, PyObject>,
}

#[pymethods]
impl SyncGroup {
    #[getter]
    fn timestamp_ns(&self) -> i64 {
        self.timestamp_ns
    }

    fn get(&self, py: Python, topic: &str) -> Option<PyObject> {
        self.messages.get(topic).map(|m| m.clone_ref(py))
    }

    fn topics(&self) -> Vec<String> {
        self.messages.keys().cloned().collect()
    }

    fn __getitem__(&self, py: Python, topic: &str) -> PyResult<PyObject> {
        self.get(py, topic)
            .ok_or_else(|| PyErr::new::<pyo3::exceptions::PyKeyError, _>(topic.to_string()))
    }
}

#[pyclass]
struct Synchronizer {
    inner: conflux_core::State<String, Message>,
    // ...
}

#[pymethods]
impl Synchronizer {
    #[new]
    fn new(topics: Vec<String>, config: &SyncConfig) -> PyResult<Self> {
        // Initialize synchronizer
    }

    fn push(&mut self, topic: &str, timestamp_ns: i64, message: PyObject) -> PyResult<()> {
        // Add message to synchronizer
    }

    fn poll(&mut self, py: Python) -> PyResult<Option<SyncGroup>> {
        // Check for synchronized groups
    }

    fn __iter__(slf: PyRef<Self>) -> PyRef<Self> {
        slf
    }

    fn __next__(&mut self, py: Python) -> PyResult<Option<SyncGroup>> {
        self.poll(py)
    }
}

#[pymodule]
fn _conflux_py(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<SyncConfig>()?;
    m.add_class::<SyncGroup>()?;
    m.add_class::<Synchronizer>()?;
    Ok(())
}
```

### Python Wrapper (conflux_py/synchronizer.py)

```python
"""High-level synchronization API for ROS2 Python nodes."""

from typing import Callable, Dict, List, Optional, Any
import rclpy
from rclpy.node import Node
from rclpy.qos import QoSProfile, QoSReliabilityPolicy, QoSHistoryPolicy

from ._conflux_py import Synchronizer as _Synchronizer, SyncConfig, SyncGroup


class Synchronizer:
    """Multi-stream message synchronizer for ROS2.

    Example:
        >>> sync = Synchronizer(node, window_size_ms=50)
        >>> sync.add_subscription(Image, "/camera/image")
        >>> sync.add_subscription(PointCloud2, "/lidar/points")
        >>>
        >>> @sync.on_synchronized
        >>> def callback(group):
        ...     image = group["/camera/image"]
        ...     points = group["/lidar/points"]
        ...     process(image, points)
    """

    def __init__(
        self,
        node: Node,
        window_size_ms: int = 50,
        buffer_size: int = 64,
        qos: Optional[QoSProfile] = None,
    ):
        self._node = node
        self._config = SyncConfig(window_size_ms, buffer_size)
        self._topics: List[str] = []
        self._subscriptions = []
        self._callback: Optional[Callable[[SyncGroup], None]] = None
        self._sync: Optional[_Synchronizer] = None

        if qos is None:
            self._qos = QoSProfile(
                reliability=QoSReliabilityPolicy.BEST_EFFORT,
                history=QoSHistoryPolicy.KEEP_LAST,
                depth=1,
            )
        else:
            self._qos = qos

    def add_subscription(self, msg_type: type, topic: str) -> None:
        """Add a topic to synchronize."""
        self._topics.append(topic)

        def msg_callback(msg, topic=topic):
            if self._sync is not None:
                # Extract timestamp from header
                stamp = msg.header.stamp
                timestamp_ns = stamp.sec * 1_000_000_000 + stamp.nanosec
                self._sync.push(topic, timestamp_ns, msg)
                self._poll()

        sub = self._node.create_subscription(
            msg_type, topic, msg_callback, self._qos
        )
        self._subscriptions.append(sub)

    def on_synchronized(self, callback: Callable[[SyncGroup], None]) -> Callable:
        """Register callback for synchronized message groups.

        Can be used as a decorator:
            @sync.on_synchronized
            def handle_sync(group):
                ...
        """
        self._callback = callback
        # Initialize synchronizer now that we have callback
        if self._sync is None and self._topics:
            self._sync = _Synchronizer(self._topics, self._config)
        return callback

    def _poll(self) -> None:
        """Check for synchronized groups and invoke callback."""
        if self._sync is None or self._callback is None:
            return

        while True:
            group = self._sync.poll()
            if group is None:
                break
            self._callback(group)
```

### Usage Example

```python
#!/usr/bin/env python3
"""Example ROS2 node using conflux_py for synchronization."""

import rclpy
from rclpy.node import Node
from sensor_msgs.msg import Image, PointCloud2
from conflux_py import Synchronizer


class SyncProcessorNode(Node):
    def __init__(self):
        super().__init__("sync_processor")

        # Create synchronizer with 50ms window
        self.sync = Synchronizer(self, window_size_ms=50)

        # Add topics to synchronize
        self.sync.add_subscription(Image, "/camera/image")
        self.sync.add_subscription(PointCloud2, "/lidar/points")

        # Register callback
        @self.sync.on_synchronized
        def on_sync(group):
            image = group["/camera/image"]
            points = group["/lidar/points"]
            self.get_logger().info(
                f"Synchronized: image={image.header.stamp}, "
                f"points={points.header.stamp}"
            )
            self.process(image, points)

    def process(self, image: Image, points: PointCloud2):
        # Your processing logic here
        pass


def main():
    rclpy.init()
    node = SyncProcessorNode()
    rclpy.spin(node)
    node.destroy_node()
    rclpy.shutdown()


if __name__ == "__main__":
    main()
```

## setup.py

```python
from setuptools import setup, find_packages
from setuptools_rust import Binding, RustExtension

package_name = "conflux_py"

setup(
    name=package_name,
    version="0.2.0",
    packages=find_packages(exclude=["test"]),
    rust_extensions=[
        RustExtension(
            f"{package_name}._conflux_py",
            path="rust/Cargo.toml",
            binding=Binding.PyO3,
        )
    ],
    data_files=[
        ("share/ament_index/resource_index/packages", [f"resource/{package_name}"]),
        (f"share/{package_name}", ["package.xml"]),
    ],
    install_requires=["setuptools", "setuptools-rust"],
    zip_safe=False,
    maintainer="TODO",
    maintainer_email="todo@example.com",
    description="Python library for multi-stream message synchronization",
    license="MIT OR Apache-2.0",
    tests_require=["pytest"],
    entry_points={
        "console_scripts": [
            f"example_sync_node = {package_name}.examples.sync_node:main",
        ],
    },
)
```

## package.xml

```xml
<?xml version="1.0"?>
<package format="3">
  <name>conflux_py</name>
  <version>0.2.0</version>
  <description>Python library for multi-stream message synchronization</description>

  <maintainer email="todo@example.com">TODO</maintainer>
  <license>MIT</license>
  <license>Apache-2.0</license>

  <buildtool_depend>ament_python</buildtool_depend>
  <buildtool_depend>python3-setuptools-rust</buildtool_depend>

  <depend>rclpy</depend>
  <depend>std_msgs</depend>

  <test_depend>ament_copyright</test_depend>
  <test_depend>ament_flake8</test_depend>
  <test_depend>ament_pep257</test_depend>
  <test_depend>python3-pytest</test_depend>

  <export>
    <build_type>ament_python</build_type>
  </export>
</package>
```

## rust/Cargo.toml

```toml
[package]
name = "conflux-py"
version = "0.2.0"
edition = "2021"

[lib]
name = "_conflux_py"
crate-type = ["cdylib"]

[dependencies]
conflux-core = { path = "../../crates/conflux-core", features = ["tokio"] }
pyo3 = { version = "0.20", features = ["extension-module"] }
```

## Implementation Steps

1. Create `conflux_py/` directory structure
2. Implement PyO3 native module (`rust/src/lib.rs`)
3. Create Python wrapper (`conflux_py/synchronizer.py`)
4. Set up setup.py with setuptools-rust
5. Add type stubs for IDE support
6. Create example node
7. Add pytest tests

## Verification Checklist

- [x] `maturin build --release` succeeds
- [x] Native module imports correctly: `from conflux_py._conflux_py import ...`
- [x] Python wrapper works with rclpy (ROS2Synchronizer class)
- [x] Example node provided (`examples/sync_node.py`)
- [x] Type hints work in IDE (type stubs provided)
- [x] pytest tests pass (19 tests)

## Dependencies

- Rust toolchain (for building PyO3 crate)
- maturin (Python package for building)
- pyo3 (Rust crate)
- rclpy (ROS2 Python client library, optional)
