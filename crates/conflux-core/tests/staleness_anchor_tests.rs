//! H-11: staleness expiry must be measured from when a message is tracked, not
//! from when the detector was constructed.

use conflux_core::{StalenessConfig, WithTimestamp, staleness::ConstrainedHeap};
use std::{thread::sleep, time::Duration};

#[derive(Debug, Clone, PartialEq, Eq)]
struct M(Duration);

impl WithTimestamp for M {
    fn timestamp(&self) -> Duration {
        self.0
    }
}

/// A message pushed long after construction must still get its full timeout.
///
/// With expiry anchored to construction time, every message arriving later than
/// `staleness_timeout` after startup is born expired -- i.e. everything, within
/// the first second of a real run.
#[test]
fn fresh_message_is_not_expired_when_detector_is_old() {
    let config = StalenessConfig {
        heap_time_horizon: Duration::from_secs(10),
        ..StalenessConfig::default()
    };
    let mut heap: ConstrainedHeap<&str, M> = ConstrainedHeap::new(config);

    // The synchronizer has been alive a while before this message arrives.
    sleep(Duration::from_millis(300));

    heap.try_add(
        "A",
        M(Duration::from_millis(1000)),
        Duration::from_millis(200),
    )
    .expect("message within the horizon should be admitted to the heap");

    assert_eq!(
        heap.drain_expired().len(),
        0,
        "a message with a 200ms timeout expired immediately, 300ms after construction"
    );
}

/// The timeout must still be honoured: once it elapses, the message expires.
#[test]
fn message_expires_after_its_own_timeout() {
    let config = StalenessConfig {
        heap_time_horizon: Duration::from_secs(10),
        ..StalenessConfig::default()
    };
    let mut heap: ConstrainedHeap<&str, M> = ConstrainedHeap::new(config);

    heap.try_add(
        "A",
        M(Duration::from_millis(1000)),
        Duration::from_millis(50),
    )
    .expect("message within the horizon should be admitted to the heap");

    assert_eq!(heap.drain_expired().len(), 0, "expired before its timeout");

    sleep(Duration::from_millis(80));

    assert_eq!(
        heap.drain_expired().len(),
        1,
        "message should expire once its timeout has elapsed"
    );
}

/// Two messages added at different times must expire in the order their own
/// deadlines fall due, not in construction order.
#[test]
fn expiry_order_follows_arrival_not_construction() {
    let config = StalenessConfig {
        heap_time_horizon: Duration::from_secs(10),
        precision_gap: Duration::from_micros(1),
        ..StalenessConfig::default()
    };
    let mut heap: ConstrainedHeap<&str, M> = ConstrainedHeap::new(config);

    // Added first, long timeout -- should outlive the one added later.
    heap.try_add(
        "A",
        M(Duration::from_millis(1000)),
        Duration::from_millis(400),
    )
    .unwrap();
    sleep(Duration::from_millis(50));
    heap.try_add(
        "B",
        M(Duration::from_millis(1050)),
        Duration::from_millis(100),
    )
    .unwrap();

    sleep(Duration::from_millis(150));

    let expired = heap.drain_expired();
    assert_eq!(expired.len(), 1, "exactly one message should have expired");
    assert_eq!(
        expired[0].0, "B",
        "the shorter-lived message should expire first"
    );
}
