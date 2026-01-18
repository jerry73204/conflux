use crate::staleness::StalenessConfig;
use std::time::Duration;

/// Policy for handling buffer overflow when pushing new messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DropPolicy {
    /// Reject new messages when buffer is full (error returned to caller).
    /// Preserves existing data. Suitable for offline/rosbag processing.
    #[default]
    RejectNew,

    /// Drop the oldest message to make room for the new one.
    /// Always accepts new data. Suitable for realtime processing.
    DropOldest,
}

/// Configuration parameters that are passed to [sync](crate::sync());
#[derive(Debug, Clone)]
pub struct Config {
    /// The time span that the grouped frames must fit within.
    /// None means infinite window (no time-based dropping).
    pub window_size: Option<Duration>,
    /// Accepted minimum timestamps for input frames.
    pub start_time: Option<Duration>,
    /// The maximum number of frames kept for each input stream.
    pub buf_size: usize,
    /// Policy for handling buffer overflow.
    pub drop_policy: DropPolicy,
    /// Staleness detection configuration (optional)
    pub staleness_config: Option<StalenessConfig>,
}

impl Config {
    /// Create a new Config with staleness detection enabled
    pub fn with_staleness(
        window_size: Option<Duration>,
        start_time: Option<Duration>,
        buf_size: usize,
        drop_policy: DropPolicy,
        staleness_config: StalenessConfig,
    ) -> Self {
        Self {
            window_size,
            start_time,
            buf_size,
            drop_policy,
            staleness_config: Some(staleness_config),
        }
    }

    /// Create a basic Config without staleness detection
    pub fn basic(
        window_size: Option<Duration>,
        start_time: Option<Duration>,
        buf_size: usize,
    ) -> Self {
        Self {
            window_size,
            start_time,
            buf_size,
            drop_policy: DropPolicy::default(),
            staleness_config: None,
        }
    }

    /// Create config for offline processing (rosbag playback).
    /// Uses infinite window and RejectNew policy to preserve all data.
    pub fn offline(buf_size: usize) -> Self {
        Self {
            window_size: None,
            start_time: None,
            buf_size,
            drop_policy: DropPolicy::RejectNew,
            staleness_config: None,
        }
    }

    /// Create config for realtime processing (live sensors).
    /// Uses finite window and DropOldest policy to always process latest data.
    pub fn realtime(window_size: Duration, buf_size: usize) -> Self {
        Self {
            window_size: Some(window_size),
            start_time: None,
            buf_size,
            drop_policy: DropPolicy::DropOldest,
            staleness_config: None,
        }
    }

    /// Set the drop policy
    pub fn with_drop_policy(mut self, drop_policy: DropPolicy) -> Self {
        self.drop_policy = drop_policy;
        self
    }

    /// Enable staleness detection on an existing config
    pub fn enable_staleness(mut self, staleness_config: StalenessConfig) -> Self {
        self.staleness_config = Some(staleness_config);
        self
    }
}
