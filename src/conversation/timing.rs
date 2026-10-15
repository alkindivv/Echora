//! Local turn timing, including the boundary between work and the final reply.
use std::{collections::HashMap, time::Instant};

#[derive(Default)]
pub(crate) struct LiveTurnTiming {
    started: Option<Instant>,
    finished: Option<Instant>,
    messages: HashMap<String, Instant>,
}

impl LiveTurnTiming {
    pub(crate) fn start(&mut self, now: Instant) {
        *self = Self {
            started: Some(now),
            ..Self::default()
        };
    }

    pub(crate) fn message_started(&mut self, id: &str, now: Instant) {
        self.messages.entry(id.to_owned()).or_insert(now);
    }

    pub(crate) fn finish(&mut self, now: Instant) {
        if self.started.is_some() {
            self.finished.get_or_insert(now);
        }
    }

    pub(crate) fn work_duration_ms(&self, answer: Option<&str>, now: Instant) -> Option<i64> {
        let start = self.started?;
        let end = answer
            .and_then(|id| self.messages.get(id).copied())
            .or(self.finished)
            .unwrap_or(now);
        Some(
            end.saturating_duration_since(start)
                .as_millis()
                .min(i64::MAX as u128) as i64,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn work_time_freezes_at_the_answer_start_and_resets_for_the_next_turn() {
        let now = Instant::now();
        let mut timing = LiveTurnTiming::default();
        timing.start(now);
        timing.message_started("commentary", now + Duration::from_secs(1));
        timing.message_started("answer", now + Duration::from_secs(3));
        timing.message_started("answer", now + Duration::from_secs(4));
        assert_eq!(
            timing.work_duration_ms(None, now + Duration::from_secs(2)),
            Some(2000)
        );
        assert_eq!(
            timing.work_duration_ms(Some("answer"), now + Duration::from_secs(7)),
            Some(3000)
        );
        timing.finish(now + Duration::from_secs(8));
        assert_eq!(
            timing.work_duration_ms(None, now + Duration::from_secs(9)),
            Some(8000)
        );
        timing.start(now + Duration::from_secs(10));
        assert_eq!(
            timing.work_duration_ms(Some("answer"), now + Duration::from_secs(11)),
            Some(1000)
        );
    }
}
