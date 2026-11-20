use std::time::{Duration, Instant};

// PostgreSQL grants 15 seconds. Count from request dispatch and relinquish two
// seconds early to allow for scheduling delays; only a conditional SQL update
// confirmed within the previous window can start a new window.
const CONFIRMED_WINDOW: Duration = Duration::from_secs(13);
const RENEW_AFTER: Duration = Duration::from_secs(3);
const ATTEMPT_LIMIT: Duration = Duration::from_secs(3);
const RETRY_AFTER: Duration = Duration::from_millis(500);

pub(super) struct LeaseSchedule {
    pub(super) stop_at: Instant,
    pub(super) next_attempt: Instant,
}

impl LeaseSchedule {
    pub(super) fn confirmed(sent: Instant) -> Self {
        Self {
            stop_at: sent + CONFIRMED_WINDOW,
            next_attempt: sent + RENEW_AFTER,
        }
    }

    pub(super) fn attempt_deadline(&self, now: Instant) -> Option<Instant> {
        (now < self.stop_at).then(|| self.stop_at.min(now + ATTEMPT_LIMIT))
    }

    pub(super) fn retry(&mut self, now: Instant) {
        self.next_attempt = self.stop_at.min(now + RETRY_AFTER);
    }

    pub(super) fn acknowledge(
        &mut self,
        rows: u64,
        sent: Instant,
        received: Instant,
    ) -> Result<(), &'static str> {
        if rows != 1 {
            return Err("database no longer confirms this holder and generation");
        }
        if received >= self.stop_at {
            return Err("database confirmation arrived after the safety deadline");
        }
        *self = Self::confirmed(sent);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slow_startup_and_repeated_failures_share_one_deadline() {
        let start = Instant::now();
        let mut lease = LeaseSchedule::confirmed(start);
        let startup_finished = start + Duration::from_secs(9);
        assert!(lease.next_attempt < startup_finished);
        assert_eq!(
            lease.attempt_deadline(startup_finished),
            Some(start + Duration::from_secs(12))
        );
        lease.retry(start + Duration::from_secs(12));
        assert_eq!(lease.next_attempt, start + Duration::from_millis(12500));
        assert_eq!(
            lease.attempt_deadline(lease.next_attempt),
            Some(start + Duration::from_secs(13))
        );
        lease.retry(start + Duration::from_millis(12900));
        assert_eq!(lease.next_attempt, start + Duration::from_secs(13));
        assert_eq!(lease.attempt_deadline(lease.next_attempt), None);
    }

    #[test]
    fn only_an_in_time_confirmation_of_the_same_owner_advances_the_deadline() {
        let start = Instant::now();
        let mut lease = LeaseSchedule::confirmed(start);
        let sent = start + Duration::from_secs(6);
        let received = sent + Duration::from_secs(2);
        let original_deadline = lease.stop_at;
        assert!(lease.acknowledge(0, sent, received).is_err());
        assert!(lease.acknowledge(1, sent, original_deadline).is_err());
        assert_eq!(lease.stop_at, original_deadline);
        lease.acknowledge(1, sent, received).unwrap();
        assert_eq!(lease.stop_at, sent + CONFIRMED_WINDOW);
        assert_eq!(lease.next_attempt, sent + RENEW_AFTER);
    }
}
