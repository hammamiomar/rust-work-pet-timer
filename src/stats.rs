use crate::data::{Session, SessionType};
use chrono::{Duration, NaiveDate};

/// A day counts toward the streak with at least this much work.
const STREAK_MIN_WORK_SECS: i64 = 25 * 60;

#[derive(Debug, Clone, Copy)]
pub struct DayStat {
    pub date: NaiveDate,
    pub work: Duration,
    pub brk: Duration,
}

impl DayStat {
    pub fn ratio(&self) -> f64 {
        let w = self.work.num_seconds() as f64;
        let b = self.brk.num_seconds() as f64;
        if w + b > 0.0 { w / (w + b) } else { 0.0 }
    }
}

pub fn day_stat(sessions: &[Session], date: NaiveDate) -> DayStat {
    let mut work = Duration::zero();
    let mut brk = Duration::zero();
    for s in sessions
        .iter()
        .filter(|s| s.start_time_local().date_naive() == date)
    {
        match s.session_type {
            SessionType::Work => work += s.duration(),
            SessionType::Break => brk += s.duration(),
            SessionType::Idle => {}
        }
    }
    DayStat { date, work, brk }
}

/// Stats for the `n` days ending at `end` (oldest first).
pub fn last_n_days(sessions: &[Session], end: NaiveDate, n: usize) -> Vec<DayStat> {
    (0..n)
        .rev()
        .map(|back| day_stat(sessions, end - Duration::days(back as i64)))
        .collect()
}

/// Consecutive days with meaningful work, counting back from `today`.
/// Today itself doesn't break the streak while it's still in progress.
pub fn streak(sessions: &[Session], today: NaiveDate) -> u32 {
    let mut count = 0;
    let mut date = today;
    // An unfinished today shouldn't zero the streak; skip it unless it qualifies.
    if day_stat(sessions, date).work.num_seconds() >= STREAK_MIN_WORK_SECS {
        count += 1;
    }
    date -= Duration::days(1);
    while day_stat(sessions, date).work.num_seconds() >= STREAK_MIN_WORK_SECS {
        count += 1;
        date -= Duration::days(1);
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Local, TimeZone, Utc};

    fn work_session_on(date: NaiveDate, hours: i64) -> Session {
        let start_local = Local
            .from_local_datetime(&date.and_hms_opt(9, 0, 0).unwrap())
            .unwrap();
        let start = start_local.with_timezone(&Utc);
        let mut s = Session::new(SessionType::Work, start);
        s.end_time = Some(start + Duration::hours(hours));
        s
    }

    #[test]
    fn day_stat_sums_work() {
        let today = Local::now().date_naive();
        let sessions = vec![work_session_on(today, 2), work_session_on(today, 1)];
        assert_eq!(day_stat(&sessions, today).work, Duration::hours(3));
    }

    #[test]
    fn streak_counts_consecutive_days() {
        let today = Local::now().date_naive();
        let sessions = vec![
            work_session_on(today, 1),
            work_session_on(today - Duration::days(1), 1),
            work_session_on(today - Duration::days(2), 1),
            // gap on day 3
            work_session_on(today - Duration::days(4), 1),
        ];
        assert_eq!(streak(&sessions, today), 3);
    }

    #[test]
    fn streak_survives_quiet_morning() {
        let today = Local::now().date_naive();
        let sessions = vec![
            work_session_on(today - Duration::days(1), 1),
            work_session_on(today - Duration::days(2), 1),
        ];
        // Nothing logged today yet — streak should still show yesterday's run.
        assert_eq!(streak(&sessions, today), 2);
    }

    #[test]
    fn last_n_days_is_oldest_first() {
        let today = Local::now().date_naive();
        let days = last_n_days(&[], today, 7);
        assert_eq!(days.len(), 7);
        assert_eq!(days[6].date, today);
        assert_eq!(days[0].date, today - Duration::days(6));
    }
}
