//! This library group up messages close in time within a time window
//! from multiple message streams, each identified by a key.
//!
//! # Usage
//!
//! ```rust
//! use conflux_core::{Config, WithTimestamp, sync};
//! use futures::{
//!     stream,
//!     stream::{StreamExt, TryStreamExt},
//! };
//! use indexmap::IndexMap;
//! use std::time::Duration;
//!
//! // Define your message type
//! #[derive(Clone)]
//! struct MyMessage(Duration);
//!
//! impl WithTimestamp for MyMessage {
//!     fn timestamp(&self) -> Duration {
//!         self.0
//!     }
//! }
//!
//! # #[tokio::main]
//! # async fn main() -> eyre::Result<()> {
//! // Create two message streams
//! let stream_x = stream::iter([
//!     MyMessage(Duration::from_millis(1001)),
//!     MyMessage(Duration::from_millis(1999)),
//!     MyMessage(Duration::from_millis(3000)),
//! ]);
//! let stream_y = stream::iter([
//!     MyMessage(Duration::from_millis(998)),
//!     MyMessage(Duration::from_millis(2003)),
//!     MyMessage(Duration::from_millis(3002)),
//! ]);
//!
//! // Join two streams into one, where each message is identified by
//! // key.
//! let join_stream = stream::select(
//!     stream_x.map(|msg| ("X", msg)),
//!     stream_y.map(|msg| ("Y", msg)),
//! )
//! .map(|msg| eyre::Ok(msg));
//!
//! // Run the synchronization algorithm
//! let config = Config {
//!     window_size: Duration::from_millis(500),
//!     start_time: None,
//!     buf_size: 16,
//!     staleness_config: None,
//! };
//! let (sync_stream, feedback_stream) = sync(join_stream, ["X", "Y"], config)?;
//!
//! // Collect the groups
//! let groups: Vec<IndexMap<&str, MyMessage>> = sync_stream.try_collect().await?;
//! # Ok(())
//! # }
//! ```

pub mod buffer;
mod config;
pub mod staleness;
pub mod state;
mod sync;
mod types;
mod utils;

pub use config::Config;
pub use staleness::{StalenessConfig, StalenessDetector, StalenessStats};
pub use sync::sync;
pub use types::*;
