//! H-12: the `sync()` stream driver and the C FFI driver must agree.
//!
//! Both now route through `State::advance`, which owns the "match, or force
//! progress when waiting is futile" rule. These tests pin `sync()`'s side of
//! that contract; the FFI side is covered in `conflux-ffi`'s own tests
//! (`test_recovers_from_divergence_*`).

use conflux_core::{Config, DropPolicy, WithTimestamp, sync};
use futures::{StreamExt, TryStreamExt, stream};
use indexmap::IndexMap;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Msg(Duration);

impl WithTimestamp for Msg {
    fn timestamp(&self) -> Duration {
        self.0
    }
}

fn ms(v: u64) -> Msg {
    Msg(Duration::from_millis(v))
}

/// The C-05 scenario driven through `sync()`: two streams diverge past any
/// common window, then perfectly aligned data arrives. The pipeline must
/// recover and emit groups from the aligned tail.
#[tokio::test]
async fn sync_recovers_from_stream_divergence() {
    let mut items: Vec<eyre::Result<(&str, Msg)>> = vec![
        Ok(("A", ms(1000))),
        Ok(("A", ms(1010))),
        Ok(("B", ms(5000))),
        Ok(("B", ms(5010))),
    ];
    for i in 0..20u64 {
        let t = 6000 + i * 33;
        items.push(Ok(("A", ms(t))));
        items.push(Ok(("B", ms(t + 5))));
    }

    let config = Config::basic(Some(Duration::from_millis(50)), None, 2);
    let (out, _fb) = sync(stream::iter(items).boxed(), vec!["A", "B"], config).unwrap();

    let groups: Vec<IndexMap<&str, Msg>> = out.try_collect().await.unwrap();

    assert!(
        !groups.is_empty(),
        "sync() emitted nothing after a stream divergence"
    );

    // Every emitted group must pair messages inside the window.
    for g in &groups {
        let lo = g.values().map(|m| m.timestamp()).min().unwrap();
        let hi = g.values().map(|m| m.timestamp()).max().unwrap();
        assert!(
            hi - lo <= Duration::from_millis(50),
            "group spans {:?}, wider than the 50ms window: {g:?}",
            hi - lo
        );
    }
}

/// Clean aligned input must not lose data to the forced-progress path.
#[tokio::test]
async fn sync_emits_every_aligned_pair() {
    let mut items: Vec<eyre::Result<(&str, Msg)>> = Vec::new();
    for i in 0..12u64 {
        let t = 1000 + i * 33;
        items.push(Ok(("A", ms(t))));
        items.push(Ok(("B", ms(t + 5))));
    }

    let config = Config::basic(Some(Duration::from_millis(50)), None, 8);
    let (out, _fb) = sync(stream::iter(items).boxed(), vec!["A", "B"], config).unwrap();
    let groups: Vec<IndexMap<&str, Msg>> = out.try_collect().await.unwrap();

    assert_eq!(groups.len(), 12, "expected one group per aligned pair");
}

/// DropOldest must not wedge either: every push is accepted, so groups must flow.
#[tokio::test]
async fn sync_drop_oldest_recovers_from_divergence() {
    let mut items: Vec<eyre::Result<(&str, Msg)>> = vec![
        Ok(("A", ms(1000))),
        Ok(("A", ms(1010))),
        Ok(("B", ms(5000))),
        Ok(("B", ms(5010))),
    ];
    for i in 0..20u64 {
        let t = 6000 + i * 33;
        items.push(Ok(("A", ms(t))));
        items.push(Ok(("B", ms(t + 5))));
    }

    let config = Config {
        window_size: Some(Duration::from_millis(50)),
        start_time: None,
        buf_size: 2,
        drop_policy: DropPolicy::DropOldest,
        staleness_config: None,
    };
    let (out, _fb) = sync(stream::iter(items).boxed(), vec!["A", "B"], config).unwrap();
    let groups: Vec<IndexMap<&str, Msg>> = out.try_collect().await.unwrap();

    assert!(
        !groups.is_empty(),
        "DropOldest sync() emitted nothing after a divergence"
    );
}
