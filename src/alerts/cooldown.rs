use chrono::{DateTime, Duration, Utc};

/// Default alert cooldown: 30 minutes (1800 seconds)
#[allow(dead_code)]
pub const DEFAULT_COOLDOWN_SECONDS: u32 = 1800;

/// Utility for tracking and verifying alert cooldowns
#[derive(Debug, Clone, Copy, Default)]
pub struct CooldownTracker;

impl CooldownTracker {
    /// Checks whether an alert is currently in a cooldown state.
    /// Returns `true` if cooldown has NOT elapsed, `false` if it is eligible to trigger.
    pub fn is_cooling_down(
        last_triggered_at: Option<DateTime<Utc>>,
        cooldown_seconds: u32,
        now: DateTime<Utc>,
    ) -> bool {
        match last_triggered_at {
            None => false,
            Some(last) => {
                let duration = Duration::seconds(cooldown_seconds as i64);
                let expires_at = last + duration;
                now < expires_at
            }
        }
    }

    /// Computes the remaining time before cooldown expires.
    /// Returns `None` if cooldown has already expired or alert has never triggered.
    pub fn time_remaining(
        last_triggered_at: Option<DateTime<Utc>>,
        cooldown_seconds: u32,
        now: DateTime<Utc>,
    ) -> Option<Duration> {
        match last_triggered_at {
            None => None,
            Some(last) => {
                let duration = Duration::seconds(cooldown_seconds as i64);
                let expires_at = last + duration;
                if now < expires_at {
                    Some(expires_at - now)
                } else {
                    None
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_never_triggered_not_cooling_down() {
        let now = Utc::now();
        assert!(!CooldownTracker::is_cooling_down(None, 1800, now));
        assert_eq!(CooldownTracker::time_remaining(None, 1800, now), None);
    }

    #[test]
    fn test_cooling_down_active() {
        let now = Utc::now();
        let last = now - Duration::minutes(10); // 10 mins ago
        let cooldown = 1800; // 30 mins

        assert!(CooldownTracker::is_cooling_down(Some(last), cooldown, now));
        let rem = CooldownTracker::time_remaining(Some(last), cooldown, now).unwrap();
        assert_eq!(rem.num_minutes(), 20);
    }

    #[test]
    fn test_cooldown_expired() {
        let now = Utc::now();
        let last = now - Duration::minutes(31); // 31 mins ago
        let cooldown = 1800; // 30 mins

        assert!(!CooldownTracker::is_cooling_down(Some(last), cooldown, now));
        assert_eq!(CooldownTracker::time_remaining(Some(last), cooldown, now), None);
    }
}
