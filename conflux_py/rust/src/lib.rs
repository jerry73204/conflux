//! Python bindings for conflux synchronization library.
//!
//! This module provides Python-compatible wrappers around the conflux-core
//! synchronization algorithm using PyO3.

use conflux_core::{WithTimestamp, buffer::Buffer, state::State};
use indexmap::IndexMap;
use pyo3::{prelude::*, types::PyDict};
use std::{collections::HashMap, time::Duration};

/// Internal message wrapper that implements WithTimestamp and holds a Python object.
struct PyMessage {
    timestamp: Duration,
    object: PyObject,
}

impl Clone for PyMessage {
    fn clone(&self) -> Self {
        Python::with_gil(|py| Self {
            timestamp: self.timestamp,
            object: self.object.clone_ref(py),
        })
    }
}

impl WithTimestamp for PyMessage {
    fn timestamp(&self) -> Duration {
        self.timestamp
    }
}

/// Configuration for the synchronizer.
#[pyclass]
#[derive(Clone)]
pub struct SyncConfig {
    /// Time window in milliseconds for grouping messages.
    #[pyo3(get, set)]
    pub window_size_ms: u64,
    /// Maximum number of messages to buffer per stream.
    #[pyo3(get, set)]
    pub buffer_size: usize,
}

#[pymethods]
impl SyncConfig {
    #[new]
    #[pyo3(signature = (window_size_ms=50, buffer_size=64))]
    fn new(window_size_ms: u64, buffer_size: usize) -> Self {
        Self {
            window_size_ms,
            buffer_size,
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "SyncConfig(window_size_ms={}, buffer_size={})",
            self.window_size_ms, self.buffer_size
        )
    }
}

/// A synchronized group of messages from multiple topics.
#[pyclass]
pub struct SyncGroup {
    /// Timestamp in nanoseconds for the group.
    timestamp_ns: i64,
    /// Map of topic names to messages.
    messages: HashMap<String, PyObject>,
}

#[pymethods]
impl SyncGroup {
    /// Get the timestamp of this synchronized group in nanoseconds.
    #[getter]
    fn timestamp_ns(&self) -> i64 {
        self.timestamp_ns
    }

    /// Get the timestamp of this synchronized group in seconds (float).
    #[getter]
    fn timestamp(&self) -> f64 {
        self.timestamp_ns as f64 / 1_000_000_000.0
    }

    /// Get a message by topic name.
    fn get(&self, py: Python<'_>, topic: &str) -> Option<PyObject> {
        self.messages.get(topic).map(|m| m.clone_ref(py))
    }

    /// Get all topic names in this group.
    fn topics(&self) -> Vec<String> {
        self.messages.keys().cloned().collect()
    }

    /// Get the number of messages in this group.
    fn __len__(&self) -> usize {
        self.messages.len()
    }

    /// Check if a topic is in this group.
    fn __contains__(&self, topic: &str) -> bool {
        self.messages.contains_key(topic)
    }

    /// Get a message by topic name (subscript operator).
    fn __getitem__(&self, py: Python<'_>, topic: &str) -> PyResult<PyObject> {
        self.messages
            .get(topic)
            .map(|m| m.clone_ref(py))
            .ok_or_else(|| pyo3::exceptions::PyKeyError::new_err(topic.to_string()))
    }

    /// Convert to a dictionary.
    fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let dict = PyDict::new(py);
        for (key, value) in &self.messages {
            dict.set_item(key, value.clone_ref(py))?;
        }
        Ok(dict)
    }

    fn __repr__(&self) -> String {
        let topics: Vec<_> = self.messages.keys().collect();
        format!(
            "SyncGroup(timestamp_ns={}, topics={:?})",
            self.timestamp_ns, topics
        )
    }
}

/// Multi-stream message synchronizer.
///
/// The Synchronizer collects messages from multiple streams (identified by topic names)
/// and outputs groups of messages that fall within a configurable time window.
#[pyclass]
pub struct Synchronizer {
    state: State<String, PyMessage>,
    topics: Vec<String>,
}

#[pymethods]
impl Synchronizer {
    /// Create a new synchronizer.
    ///
    /// Args:
    ///     topics: List of topic names to synchronize.
    ///     config: Optional configuration. Uses defaults if not provided.
    ///
    /// Raises:
    ///     ValueError: If topics list is empty or buffer_size < 2.
    #[new]
    #[pyo3(signature = (topics, config=None))]
    fn new(topics: Vec<String>, config: Option<&SyncConfig>) -> PyResult<Self> {
        if topics.is_empty() {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "topics list cannot be empty",
            ));
        }

        let config = config.cloned().unwrap_or_else(|| SyncConfig::new(50, 64));

        if config.buffer_size < 2 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "buffer_size must be at least 2",
            ));
        }

        // Create buffers for each topic
        let buffers: IndexMap<String, Buffer<PyMessage>> = topics
            .iter()
            .map(|topic| (topic.clone(), Buffer::with_capacity(config.buffer_size)))
            .collect();

        let state = State {
            buffers,
            commit_ts: None,
            buf_size: config.buffer_size,
            window_size: Duration::from_millis(config.window_size_ms),
            feedback_tx: None,
            staleness_detector: None,
        };

        Ok(Self {
            state,
            topics: topics.clone(),
        })
    }

    /// Push a message to the synchronizer.
    ///
    /// Args:
    ///     topic: The topic name for this message.
    ///     timestamp_ns: Timestamp in nanoseconds.
    ///     message: The message object (any Python object).
    ///
    /// Returns:
    ///     True if the message was accepted, False if rejected (buffer full or late message).
    ///
    /// Raises:
    ///     KeyError: If the topic was not registered at creation.
    ///     ValueError: If timestamp_ns is negative.
    fn push(&mut self, topic: &str, timestamp_ns: i64, message: PyObject) -> PyResult<bool> {
        if !self.topics.contains(&topic.to_string()) {
            return Err(pyo3::exceptions::PyKeyError::new_err(format!(
                "unknown topic: {}",
                topic
            )));
        }

        if timestamp_ns < 0 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "timestamp_ns must be non-negative",
            ));
        }

        let msg = PyMessage {
            timestamp: Duration::from_nanos(timestamp_ns as u64),
            object: message,
        };

        match self.state.push(topic.to_string(), msg) {
            Ok(()) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    /// Poll for a synchronized group of messages.
    ///
    /// Returns:
    ///     A SyncGroup if a synchronized group is available, None otherwise.
    fn poll(&mut self) -> Option<SyncGroup> {
        self.state.try_match().map(|group| {
            let timestamp_ns = group
                .values()
                .map(|m| m.timestamp.as_nanos() as i64)
                .min()
                .unwrap_or(0);

            let messages: HashMap<String, PyObject> = group
                .into_iter()
                .map(|(topic, msg)| (topic, msg.object))
                .collect();

            SyncGroup {
                timestamp_ns,
                messages,
            }
        })
    }

    /// Drain all available synchronized groups.
    ///
    /// Returns:
    ///     A list of all available SyncGroups.
    fn drain(&mut self) -> Vec<SyncGroup> {
        let mut groups = Vec::new();
        while let Some(group) = self.poll() {
            groups.push(group);
        }
        groups
    }

    /// Get the number of registered topics.
    #[getter]
    fn topic_count(&self) -> usize {
        self.topics.len()
    }

    /// Get the list of registered topics.
    #[getter]
    fn topics(&self) -> Vec<String> {
        self.topics.clone()
    }

    /// Check if the synchronizer is ready (all buffers have at least 2 messages).
    fn is_ready(&self) -> bool {
        self.state.is_ready()
    }

    /// Check if any buffer is empty.
    fn is_empty(&self) -> bool {
        self.state.is_empty()
    }

    /// Get the buffer length for a specific topic.
    fn buffer_len(&self, topic: &str) -> usize {
        self.state.buffers.get(topic).map(|b| b.len()).unwrap_or(0)
    }

    /// Iterate over synchronized groups.
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    /// Get next synchronized group.
    fn __next__(mut slf: PyRefMut<'_, Self>) -> Option<SyncGroup> {
        slf.poll()
    }

    fn __repr__(&self) -> String {
        format!(
            "Synchronizer(topics={:?}, window_size_ms={}, buffer_size={})",
            self.topics,
            self.state.window_size.as_millis(),
            self.state.buf_size
        )
    }
}

/// Python module definition.
#[pymodule]
fn _conflux_py(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<SyncConfig>()?;
    m.add_class::<SyncGroup>()?;
    m.add_class::<Synchronizer>()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_config_default() {
        let config = SyncConfig::new(50, 64);
        assert_eq!(config.window_size_ms, 50);
        assert_eq!(config.buffer_size, 64);
    }
}
