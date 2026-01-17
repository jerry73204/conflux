use crate::types::WithTimestamp;
use std::{
    collections::BinaryHeap,
    hash::Hash,
    time::{Duration, Instant},
};

#[cfg(feature = "tokio")]
use tokio::sync::mpsc;

/// Configuration for staleness detection
#[derive(Debug, Clone)]
pub struct StalenessConfig {
    /// Maximum number of entries in the min-heap before delegating to timer wheel
    pub heap_max_size: usize,
    /// Maximum time horizon for heap entries (messages beyond this use timer wheel)
    pub heap_time_horizon: Duration,
    /// Precision gap - minimum time between expiration checks
    pub precision_gap: Duration,
    /// Timer wheel settings for overflow handling
    pub timer_wheel_slots: usize,
    pub timer_wheel_slot_duration: Duration,
    /// Enable immediate expiration (requires tokio feature)
    pub enable_immediate_expiration: bool,
}

impl Default for StalenessConfig {
    fn default() -> Self {
        Self {
            heap_max_size: 256,
            heap_time_horizon: Duration::from_millis(100),
            precision_gap: Duration::from_micros(500),
            timer_wheel_slots: 128,
            timer_wheel_slot_duration: Duration::from_millis(10),
            enable_immediate_expiration: false,
        }
    }
}

impl StalenessConfig {
    /// High-frequency streaming (sub-millisecond precision, real-time)
    pub fn high_frequency() -> Self {
        Self {
            heap_max_size: 512,
            heap_time_horizon: Duration::from_millis(50),
            precision_gap: Duration::from_micros(100),
            timer_wheel_slots: 256,
            timer_wheel_slot_duration: Duration::from_millis(5),
            enable_immediate_expiration: true,
        }
    }

    /// Low-frequency streaming (millisecond precision, near real-time)
    pub fn low_frequency() -> Self {
        Self {
            heap_max_size: 128,
            heap_time_horizon: Duration::from_millis(500),
            precision_gap: Duration::from_millis(10),
            timer_wheel_slots: 64,
            timer_wheel_slot_duration: Duration::from_millis(50),
            enable_immediate_expiration: true,
        }
    }

    /// Batch processing (relaxed precision, lazy checking)
    pub fn batch_processing() -> Self {
        Self {
            heap_max_size: 64,
            heap_time_horizon: Duration::from_secs(1),
            precision_gap: Duration::from_millis(100),
            timer_wheel_slots: 32,
            timer_wheel_slot_duration: Duration::from_millis(200),
            enable_immediate_expiration: false,
        }
    }
}

/// Entry in the staleness heap with expiration time
#[derive(Debug, Clone)]
struct StalenessEntry<K, T> {
    expiration_time: Instant,
    messages: Vec<(K, T)>, // Support coalescing multiple messages
}

impl<K, T> PartialEq for StalenessEntry<K, T> {
    fn eq(&self, other: &Self) -> bool {
        self.expiration_time == other.expiration_time
    }
}

impl<K, T> Eq for StalenessEntry<K, T> {}

impl<K, T> PartialOrd for StalenessEntry<K, T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<K, T> Ord for StalenessEntry<K, T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Reverse ordering for min-heap behavior
        other.expiration_time.cmp(&self.expiration_time)
    }
}

/// Constrained min-heap for staleness detection with size, temporal, and precision constraints
#[derive(Debug)]
pub struct ConstrainedHeap<K, T>
where
    K: Clone + Hash + Eq,
    T: WithTimestamp + Clone,
{
    heap: BinaryHeap<StalenessEntry<K, T>>,
    config: StalenessConfig,
    reference_time: Instant,
}

impl<K, T> ConstrainedHeap<K, T>
where
    K: Clone + Hash + Eq,
    T: WithTimestamp + Clone,
{
    pub fn new(config: StalenessConfig) -> Self {
        Self {
            heap: BinaryHeap::with_capacity(config.heap_max_size),
            config,
            reference_time: Instant::now(),
        }
    }

    /// Try to add a message to the constrained heap with coalescing
    /// Returns Ok(()) if added, Err(message) if it should go to timer wheel
    pub fn try_add(
        &mut self,
        key: K,
        message: T,
        staleness_timeout: Duration,
    ) -> Result<(), (K, T)> {
        let expiration_time = self.reference_time + staleness_timeout;
        let now = Instant::now();

        // Check temporal constraint
        if expiration_time.saturating_duration_since(now) > self.config.heap_time_horizon {
            return Err((key, message));
        }

        // Phase 2: Check precision gap constraint and implement coalescing
        if let Some(existing_entry) = self.find_coalescing_slot(expiration_time) {
            // Coalesce with existing entry within precision gap
            self.add_to_existing_slot(existing_entry, key, message);
            return Ok(());
        }

        // Check size constraint before creating new entry
        if self.heap.len() >= self.config.heap_max_size {
            return Err((key, message));
        }

        // No coalescing possible - create new entry
        self.heap.push(StalenessEntry {
            expiration_time,
            messages: vec![(key, message)],
        });

        Ok(())
    }

    /// Find existing slot within precision gap for coalescing
    fn find_coalescing_slot(&self, target_time: Instant) -> Option<Instant> {
        // Phase 2 implementation: find slot within precision gap
        for entry in &self.heap {
            let time_diff = if target_time >= entry.expiration_time {
                target_time.saturating_duration_since(entry.expiration_time)
            } else {
                entry.expiration_time.saturating_duration_since(target_time)
            };

            if time_diff <= self.config.precision_gap {
                return Some(entry.expiration_time);
            }
        }
        None
    }

    /// Add message to existing coalescing slot
    fn add_to_existing_slot(&mut self, target_time: Instant, key: K, message: T) {
        // Phase 2: This is a simplified implementation
        // In a real implementation, we'd need to efficiently modify heap entries
        // For now, we'll collect all entries, modify the target, and rebuild
        let mut entries: Vec<_> = self.heap.drain().collect();

        for entry in &mut entries {
            if entry.expiration_time == target_time {
                entry.messages.push((key.clone(), message.clone()));
                break;
            }
        }

        // Rebuild heap
        for entry in entries {
            self.heap.push(entry);
        }
    }

    /// Get the next expiration time, if any
    pub fn next_expiration(&self) -> Option<Instant> {
        self.heap.peek().map(|entry| entry.expiration_time)
    }

    /// Remove and return expired messages
    pub fn drain_expired(&mut self) -> Vec<(K, T)> {
        let now = Instant::now();
        let mut expired = Vec::new();

        while let Some(entry) = self.heap.peek() {
            if entry.expiration_time <= now {
                let entry = self.heap.pop().unwrap();
                expired.extend(entry.messages);
            } else {
                break;
            }
        }

        expired
    }

    /// Get current heap size
    pub fn len(&self) -> usize {
        self.heap.len()
    }

    /// Check if heap is empty
    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }

    /// Clear all entries
    pub fn clear(&mut self) {
        self.heap.clear();
    }
}

/// Phase 3: Timer wheel for handling overflow messages
#[derive(Debug)]
pub struct TimerWheel<K, T>
where
    K: Clone + Hash + Eq,
    T: WithTimestamp + Clone,
{
    slots: Vec<Vec<(K, T, Instant)>>,
    slot_duration: Duration,
    current_slot: usize,
    start_time: Instant,
}

impl<K, T> TimerWheel<K, T>
where
    K: Clone + Hash + Eq,
    T: WithTimestamp + Clone,
{
    pub fn new(slots: usize, slot_duration: Duration) -> Self {
        Self {
            slots: vec![Vec::new(); slots],
            slot_duration,
            current_slot: 0,
            start_time: Instant::now(),
        }
    }

    /// Add message to timer wheel
    pub fn add_message(&mut self, key: K, message: T, expiration_time: Instant) {
        let slots_from_now = expiration_time
            .saturating_duration_since(self.start_time)
            .as_nanos()
            / self.slot_duration.as_nanos();

        let slot_index = (self.current_slot + slots_from_now as usize) % self.slots.len();
        self.slots[slot_index].push((key, message, expiration_time));
    }

    /// Advance timer wheel and return expired messages
    pub fn advance_and_collect_expired(&mut self) -> Vec<(K, T)> {
        let now = Instant::now();
        let mut expired = Vec::new();

        // Check current slot for expired messages
        let current_messages = &mut self.slots[self.current_slot];
        current_messages.retain(|(key, message, exp_time)| {
            if *exp_time <= now {
                expired.push((key.clone(), message.clone()));
                false
            } else {
                true
            }
        });

        // Advance to next slot based on time
        let time_passed = now.saturating_duration_since(self.start_time);
        let new_slot =
            (time_passed.as_nanos() / self.slot_duration.as_nanos()) as usize % self.slots.len();
        self.current_slot = new_slot;

        expired
    }

    /// Get next slot expiration time
    pub fn next_slot_time(&self) -> Option<Instant> {
        if self.slots.iter().any(|slot| !slot.is_empty()) {
            Some(self.start_time + self.slot_duration * (self.current_slot + 1) as u32)
        } else {
            None
        }
    }
}

/// Commands for background expiration task
#[cfg(feature = "tokio")]
#[derive(Debug)]
enum ExpirationCommand {
    RescheduleCheck(Instant),
    ProcessExpired,
    #[allow(dead_code)]
    Shutdown,
}

/// Main staleness detector combining constrained heap and timer wheel
#[derive(Debug)]
pub struct StalenessDetector<K, T>
where
    K: Clone + Hash + Eq,
    T: WithTimestamp + Clone,
{
    constrained_heap: ConstrainedHeap<K, T>,
    timer_wheel: TimerWheel<K, T>,
    config: StalenessConfig,
    #[cfg(feature = "tokio")]
    _expiration_handle: Option<tokio::task::JoinHandle<()>>,
    #[cfg(feature = "tokio")]
    command_tx: Option<mpsc::UnboundedSender<ExpirationCommand>>,
}

impl<K, T> StalenessDetector<K, T>
where
    K: Clone + Hash + Eq,
    T: WithTimestamp + Clone,
{
    pub fn new(config: StalenessConfig) -> Self {
        if config.enable_immediate_expiration {
            #[cfg(not(feature = "tokio"))]
            {
                panic!(
                    "Immediate staleness expiration requires 'tokio' feature. Either enable the 'tokio' feature or set 'enable_immediate_expiration = false' for lazy checking."
                );
            }
        }

        #[cfg(feature = "tokio")]
        let (command_tx, handle) = if config.enable_immediate_expiration {
            let (tx, rx) = mpsc::unbounded_channel();
            let handle = Self::start_background_task(rx);
            (Some(tx), Some(handle))
        } else {
            (None, None)
        };

        Self {
            constrained_heap: ConstrainedHeap::new(config.clone()),
            timer_wheel: TimerWheel::new(
                config.timer_wheel_slots,
                config.timer_wheel_slot_duration,
            ),
            config,
            #[cfg(feature = "tokio")]
            _expiration_handle: handle,
            #[cfg(feature = "tokio")]
            command_tx,
        }
    }

    #[cfg(feature = "tokio")]
    fn start_background_task(
        mut rx: mpsc::UnboundedReceiver<ExpirationCommand>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut next_expiration: Option<Instant> = None;

            loop {
                let sleep_duration = if let Some(expires_at) = next_expiration {
                    expires_at.saturating_duration_since(Instant::now())
                } else {
                    Duration::from_secs(86400) // 1 day as "infinite"
                };

                tokio::select! {
                    _ = tokio::time::sleep(sleep_duration) => {
                        // Process expired messages
                        // In a real implementation, we'd need a way to communicate back to the detector
                        // For now, this is a placeholder that demonstrates the structure
                    }
                    Some(cmd) = rx.recv() => {
                        match cmd {
                            ExpirationCommand::RescheduleCheck(new_time) => {
                                if next_expiration.is_none_or(|t| new_time < t) {
                                    next_expiration = Some(new_time);
                                }
                            }
                            ExpirationCommand::ProcessExpired => {
                                // Trigger immediate processing
                            }
                            ExpirationCommand::Shutdown => {
                                break;
                            }
                        }
                    }
                }
            }
        })
    }

    /// Add a message for staleness tracking
    pub fn add_message(&mut self, key: K, message: T, staleness_timeout: Duration) {
        match self
            .constrained_heap
            .try_add(key.clone(), message.clone(), staleness_timeout)
        {
            Ok(()) => {
                // Successfully added to heap - notify background task if enabled
                #[cfg(feature = "tokio")]
                if let Some(ref tx) = self.command_tx {
                    if let Some(next_exp) = self.constrained_heap.next_expiration() {
                        let _ = tx.send(ExpirationCommand::RescheduleCheck(next_exp));
                    }
                }
            }
            Err((key, message)) => {
                // Add to timer wheel overflow
                let expiration_time = Instant::now() + staleness_timeout;
                self.timer_wheel.add_message(key, message, expiration_time);
            }
        }
    }

    /// Get next expiration time across all storage
    pub fn next_expiration(&self) -> Option<Instant> {
        let heap_next = self.constrained_heap.next_expiration();
        let wheel_next = self.timer_wheel.next_slot_time();

        match (heap_next, wheel_next) {
            (Some(h), Some(w)) => Some(h.min(w)),
            (Some(h), None) => Some(h),
            (None, Some(w)) => Some(w),
            (None, None) => None,
        }
    }

    /// Remove and return all expired messages
    pub fn drain_expired(&mut self) -> Vec<(K, T)> {
        let mut expired = self.constrained_heap.drain_expired();
        expired.extend(self.timer_wheel.advance_and_collect_expired());
        expired
    }

    /// Get statistics about current state
    pub fn stats(&self) -> StalenessStats {
        let wheel_count = self.timer_wheel.slots.iter().map(|slot| slot.len()).sum();

        StalenessStats {
            heap_size: self.constrained_heap.len(),
            timer_wheel_size: wheel_count,
            total_tracked: self.constrained_heap.len() + wheel_count,
        }
    }

    /// Trigger immediate processing of expired messages (for tokio feature)
    #[cfg(feature = "tokio")]
    pub fn trigger_expiration_check(&self) {
        if let Some(ref tx) = self.command_tx {
            let _ = tx.send(ExpirationCommand::ProcessExpired);
        }
    }

    /// Clear all tracked messages
    pub fn clear(&mut self) {
        self.constrained_heap.clear();
        self.timer_wheel = TimerWheel::new(
            self.config.timer_wheel_slots,
            self.config.timer_wheel_slot_duration,
        );
    }
}

/// Statistics about staleness detector state
#[derive(Debug, Clone, PartialEq)]
pub struct StalenessStats {
    pub heap_size: usize,
    pub timer_wheel_size: usize,
    pub total_tracked: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestMessage {
        timestamp: Duration,
        data: String,
    }

    impl TestMessage {
        fn new(timestamp_ms: u64, data: &str) -> Self {
            Self {
                timestamp: Duration::from_millis(timestamp_ms),
                data: data.to_string(),
            }
        }
    }

    impl WithTimestamp for TestMessage {
        fn timestamp(&self) -> Duration {
            self.timestamp
        }
    }

    // Phase 1 Tests: Basic functionality
    #[test]
    fn test_staleness_config_defaults() {
        let config = StalenessConfig::default();
        assert_eq!(config.heap_max_size, 256);
        assert_eq!(config.heap_time_horizon, Duration::from_millis(100));
        assert_eq!(config.precision_gap, Duration::from_micros(500));
        assert!(!config.enable_immediate_expiration);
    }

    #[test]
    fn test_staleness_config_presets() {
        let high_freq = StalenessConfig::high_frequency();
        assert_eq!(high_freq.heap_max_size, 512);
        assert_eq!(high_freq.precision_gap, Duration::from_micros(100));
        assert!(high_freq.enable_immediate_expiration);

        let batch = StalenessConfig::batch_processing();
        assert_eq!(batch.heap_max_size, 64);
        assert!(!batch.enable_immediate_expiration);
    }

    #[test]
    fn test_constrained_heap_basic_operations() {
        let config = StalenessConfig::default();
        let mut heap = ConstrainedHeap::new(config);

        assert!(heap.is_empty());
        assert_eq!(heap.len(), 0);
        assert!(heap.next_expiration().is_none());

        let msg = TestMessage::new(1000, "test");
        let result = heap.try_add("key1", msg, Duration::from_millis(100));
        assert!(result.is_ok());

        assert!(!heap.is_empty());
        assert_eq!(heap.len(), 1);
        assert!(heap.next_expiration().is_some());
    }

    #[test]
    fn test_constrained_heap_size_constraint() {
        let config = StalenessConfig {
            heap_max_size: 2,
            heap_time_horizon: Duration::from_secs(1),
            precision_gap: Duration::from_micros(1), // Very small gap to prevent coalescing
            ..StalenessConfig::default()
        };
        let mut heap = ConstrainedHeap::new(config);

        let msg1 = TestMessage::new(1000, "msg1");
        let msg2 = TestMessage::new(2000, "msg2");
        let msg3 = TestMessage::new(3000, "msg3");

        // Use different timeouts to ensure different expiration times
        assert!(
            heap.try_add("key1", msg1, Duration::from_millis(50))
                .is_ok()
        );
        assert!(
            heap.try_add("key2", msg2, Duration::from_millis(100))
                .is_ok()
        );

        // Third message should be rejected due to size constraint
        let result = heap.try_add("key3", msg3, Duration::from_millis(150));
        assert!(result.is_err());
        assert_eq!(heap.len(), 2);
    }

    #[test]
    fn test_constrained_heap_temporal_constraint() {
        let config = StalenessConfig {
            heap_time_horizon: Duration::from_millis(50),
            ..StalenessConfig::default()
        };
        let mut heap = ConstrainedHeap::new(config);

        let msg = TestMessage::new(1000, "test");

        // Message with timeout beyond temporal horizon should be rejected
        let result = heap.try_add("key1", msg, Duration::from_millis(200));
        assert!(result.is_err());
    }

    // Phase 2 Tests: Precision gap and coalescing
    #[test]
    fn test_precision_gap_coalescing() {
        let config = StalenessConfig {
            precision_gap: Duration::from_millis(10),
            ..StalenessConfig::default()
        };
        let mut heap = ConstrainedHeap::new(config);

        let msg1 = TestMessage::new(1000, "msg1");
        let msg2 = TestMessage::new(1000, "msg2");

        // Add first message
        heap.try_add("key1", msg1, Duration::from_millis(50))
            .unwrap();

        // Add second message with slightly different expiration (should coalesce)
        heap.try_add("key2", msg2, Duration::from_millis(55))
            .unwrap();

        // Should still be one entry due to coalescing
        assert_eq!(heap.len(), 1);
    }

    // Phase 3 Tests: Timer wheel
    #[test]
    fn test_timer_wheel_basic_operations() {
        let mut wheel = TimerWheel::new(10, Duration::from_millis(100));

        let msg = TestMessage::new(1000, "test");
        let exp_time = Instant::now() + Duration::from_millis(50);

        wheel.add_message("key1", msg, exp_time);

        // Should have message in wheel
        let expired = wheel.advance_and_collect_expired();
        assert!(!expired.is_empty() || wheel.slots.iter().any(|slot| !slot.is_empty()));
    }

    #[test]
    fn test_staleness_detector_integration() {
        let config = StalenessConfig::default();
        let mut detector = StalenessDetector::new(config);

        let msg = TestMessage::new(1000, "test");
        detector.add_message("key1", msg, Duration::from_millis(100));

        let stats = detector.stats();
        assert_eq!(stats.total_tracked, 1);

        assert!(detector.next_expiration().is_some());
    }

    #[test]
    fn test_staleness_detector_overflow_to_timer_wheel() {
        let config = StalenessConfig {
            heap_max_size: 1,
            heap_time_horizon: Duration::from_millis(10),
            precision_gap: Duration::from_micros(1), // Very small gap to prevent coalescing
            ..StalenessConfig::default()
        };
        let mut detector = StalenessDetector::new(config);

        let msg1 = TestMessage::new(1000, "msg1");
        let msg2 = TestMessage::new(2000, "msg2");

        // First message should go to heap
        detector.add_message("key1", msg1, Duration::from_millis(5));

        // Second message should overflow to timer wheel due to different timeout
        detector.add_message("key2", msg2, Duration::from_millis(20)); // Beyond heap_time_horizon

        let stats = detector.stats();
        assert_eq!(stats.total_tracked, 2);
        assert_eq!(stats.heap_size, 1);
        assert_eq!(stats.timer_wheel_size, 1);
    }

    #[test]
    fn test_staleness_detector_expiration() {
        let config = StalenessConfig::default();
        let mut detector = StalenessDetector::new(config);

        let msg1 = TestMessage::new(1000, "msg1");
        let msg2 = TestMessage::new(2000, "msg2");

        // Add messages with short timeouts
        detector.add_message("key1", msg1, Duration::from_millis(1));
        detector.add_message("key2", msg2, Duration::from_millis(50));

        // Wait for first to expire
        thread::sleep(Duration::from_millis(5));

        let expired = detector.drain_expired();
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].0, "key1");

        let stats = detector.stats();
        assert_eq!(stats.total_tracked, 1);
    }

    #[test]
    fn test_staleness_detector_clear() {
        let config = StalenessConfig::default();
        let mut detector = StalenessDetector::new(config);

        let msg = TestMessage::new(1000, "test");
        detector.add_message("key1", msg, Duration::from_millis(100));

        assert_eq!(detector.stats().total_tracked, 1);

        detector.clear();
        assert_eq!(detector.stats().total_tracked, 0);
        assert!(detector.next_expiration().is_none());
    }
}
