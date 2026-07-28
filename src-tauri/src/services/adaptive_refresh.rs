use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Default)]
struct ActivityState {
    last_interaction: Option<Instant>,
    last_coding: Option<Instant>,
}

static ACTIVITY: Mutex<ActivityState> = Mutex::new(ActivityState {
    last_interaction: None,
    last_coding: None,
});

pub fn record_interaction() {
    ACTIVITY
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .last_interaction = Some(Instant::now());
}

pub fn record_coding_activity() {
    ACTIVITY
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .last_coding = Some(Instant::now());
}

pub fn refresh_interval() -> Duration {
    let state = ACTIVITY
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    interval_for_ages(
        state.last_interaction.map(|at| at.elapsed()),
        state.last_coding.map(|at| at.elapsed()),
    )
}

fn interval_for_ages(interaction_age: Option<Duration>, coding_age: Option<Duration>) -> Duration {
    let freshest = [interaction_age, coding_age].into_iter().flatten().min();
    match freshest {
        Some(_) if interaction_age.is_some_and(|value| value <= Duration::from_secs(5 * 60)) => {
            Duration::from_secs(2 * 60)
        }
        Some(age) if age <= Duration::from_secs(5 * 60) => Duration::from_secs(5 * 60),
        Some(age) if age <= Duration::from_secs(60 * 60) => Duration::from_secs(5 * 60),
        Some(age) if age <= Duration::from_secs(4 * 60 * 60) => Duration::from_secs(15 * 60),
        _ => Duration::from_secs(30 * 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interaction_uses_fast_interval() {
        assert_eq!(
            interval_for_ages(Some(Duration::from_secs(60)), None),
            Duration::from_secs(120)
        );
    }

    #[test]
    fn coding_activity_uses_five_minutes() {
        assert_eq!(
            interval_for_ages(None, Some(Duration::from_secs(60))),
            Duration::from_secs(300)
        );
    }

    #[test]
    fn long_idle_backs_off_to_thirty_minutes() {
        assert_eq!(
            interval_for_ages(None, Some(Duration::from_secs(5 * 60 * 60))),
            Duration::from_secs(1800)
        );
    }
}
