use crate::staleness::StalenessConfig;
use std::time::Duration;

/// Configuration parameters that are passed to [sync](crate::sync());
#[derive(Debug, Clone)]
pub struct Config {
    /// The time span that the grouped frames must fit within.
    pub window_size: Duration,
    /// Accepted minimum timestamps for input frames.
    pub start_time: Option<Duration>,
    /// The maximum number of frames kept for each input stream.
    pub buf_size: usize,
    /// Staleness detection configuration (optional)
    pub staleness_config: Option<StalenessConfig>,
}

impl Config {
    /// Create a new Config with staleness detection enabled
    pub fn with_staleness(
        window_size: Duration,
        start_time: Option<Duration>,
        buf_size: usize,
        staleness_config: StalenessConfig,
    ) -> Self {
        Self {
            window_size,
            start_time,
            buf_size,
            staleness_config: Some(staleness_config),
        }
    }

    /// Create a basic Config without staleness detection
    pub fn basic(window_size: Duration, start_time: Option<Duration>, buf_size: usize) -> Self {
        Self {
            window_size,
            start_time,
            buf_size,
            staleness_config: None,
        }
    }

    /// Enable staleness detection on an existing config
    pub fn enable_staleness(mut self, staleness_config: StalenessConfig) -> Self {
        self.staleness_config = Some(staleness_config);
        self
    }
}
