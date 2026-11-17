//! 未采样的时钟可立即刷新，无需向前回拨 Instant。
//! 避免 Windows 刚开机时单调时钟回拨下溢（#11）。
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Default)]
pub struct PollClock(Option<Instant>);

impl PollClock {
    pub fn now() -> Self { Self(Some(Instant::now())) }

    pub fn ready(&self, interval: Duration) -> bool {
        self.ready_at(Instant::now(), interval)
    }

    fn ready_at(&self, now: Instant, interval: Duration) -> bool {
        self.0.is_none_or(|last| now.saturating_duration_since(last) >= interval)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_poll_is_ready_even_when_backdating_would_underflow() {
        let now = Instant::now();
        assert!(now.checked_sub(Duration::MAX).is_none());
        let clock = PollClock::default();
        for interval in [Duration::from_millis(100), Duration::from_millis(500),
            Duration::from_secs(1), Duration::from_secs(5), Duration::from_secs(15),
            Duration::from_secs(180), Duration::MAX] {
            assert!(clock.ready_at(now, interval));
        }
    }

    #[test]
    fn refresh_waits_until_interval_then_becomes_ready() {
        let now = Instant::now();
        let clock = PollClock(Some(now));
        let interval = Duration::from_secs(180);
        assert!(!clock.ready_at(now, interval));
        assert!(!clock.ready_at(now + interval - Duration::from_nanos(1), interval));
        assert!(clock.ready_at(now + interval, interval));
        assert!(clock.ready_at(now + interval + Duration::from_nanos(1), interval));
        assert!(!PollClock(Some(now + interval)).ready_at(now + interval, interval));
    }

    #[test]
    fn volume_echo_is_not_suppressed_at_startup_but_is_after_a_change() {
        let now = Instant::now();
        let quiet = Duration::from_millis(900);
        assert!(PollClock::default().ready_at(now, quiet));
        let clock = PollClock(Some(now));
        assert!(!clock.ready_at(now + Duration::from_millis(899), quiet));
        assert!(clock.ready_at(now + quiet, quiet));
    }

    #[test]
    fn backwards_clock_observation_does_not_panic_or_poll_early() {
        let now = Instant::now();
        let clock = PollClock(Some(now + Duration::from_secs(1)));
        assert!(!clock.ready_at(now, Duration::from_millis(100)));
    }
}
