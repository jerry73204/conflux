use conflux_core::{Config, WithTimestamp, sync};
use futures::{
    stream,
    stream::{StreamExt, TryStreamExt},
};
use indexmap::IndexMap;
use std::time::Duration;

// Define your message type
#[derive(Debug, Clone)]
struct MyMessage(Duration);

impl WithTimestamp for MyMessage {
    fn timestamp(&self) -> Duration {
        self.0
    }
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    let x_seq = &[1001, 1999, 3000];
    let y_seq = &[998, 2003, 3002];

    macro_rules! make_stream {
        ($seq:expr) => {{ stream::iter($seq.iter().map(|&ts| MyMessage(Duration::from_millis(ts)))) }};
    }

    // Create two message streams
    let stream_x = make_stream!(x_seq);
    let stream_y = make_stream!(y_seq);

    // Join two streams into one, where each message is identified by
    // key.
    let join_stream = stream::select(
        stream_x.map(|msg| ("X", msg)),
        stream_y.map(|msg| ("Y", msg)),
    )
    .map(eyre::Ok);

    // Run the synchronization algorithm
    let config = Config {
        window_size: Duration::from_millis(500),
        start_time: None,
        buf_size: 16,
        staleness_config: None,
    };
    let (sync_stream, _feedback_stream) = sync(join_stream, ["X", "Y"], config)?;

    // Collect the groups
    let groups: Vec<IndexMap<&str, MyMessage>> = sync_stream.try_collect().await?;
    println!("{groups:#?}");

    Ok(())
}
