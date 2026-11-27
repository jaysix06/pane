//! Dates as people read them: "Today 9:26 AM", "Yesterday 4:10 PM",
//! "Mon 10:37 PM" within the week, then "Nov 12" (and "Nov 12, 2025" in
//! other years).

/// A moment in local time, as the date column needs it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalTime {
    pub year: u16,
    pub month: u16,
    pub day: u16,
    pub hour: u16,
    pub minute: u16,
    /// 0 is Sunday.
    pub weekday: u16,
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/// One hour in FILETIME ticks (100 ns).
pub const HOUR: u64 = 36_000_000_000;

/// Days since 1970-01-01 for a civil date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn day_number(t: &LocalTime) -> i64 {
    days_from_civil(i64::from(t.year), i64::from(t.month), i64::from(t.day))
}

/// "9:26 AM".
pub fn clock(t: &LocalTime) -> String {
    let (hour, ampm) = match t.hour {
        0 => (12, "AM"),
        h @ 1..=11 => (h, "AM"),
        12 => (12, "PM"),
        h => (h - 12, "PM"),
    };
    format!("{hour}:{:02} {ampm}", t.minute)
}

/// `then` as people say it, seen from `now`.
pub fn relative(then: &LocalTime, now: &LocalTime) -> String {
    let days = day_number(now) - day_number(then);
    let month = MONTHS[usize::from(then.month.clamp(1, 12) - 1)];
    match days {
        0 => format!("Today {}", clock(then)),
        1 => format!("Yesterday {}", clock(then)),
        2..=6 => format!("{} {}", DAYS[usize::from(then.weekday % 7)], clock(then)),
        _ if then.year == now.year => format!("{month} {}", then.day),
        _ => format!("{month} {}, {}", then.day, then.year),
    }
}

/// The full form: "11/22/2026 3:04 PM".
pub fn absolute(t: &LocalTime) -> String {
    format!("{}/{}/{} {}", t.month, t.day, t.year, clock(t))
}

/// A FILETIME in local time (None for 0 or one Windows can't convert).
#[cfg(windows)]
pub fn local_time(filetime: u64) -> Option<LocalTime> {
    use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
    if filetime == 0 {
        return None;
    }
    let ft = FILETIME { dwLowDateTime: filetime as u32, dwHighDateTime: (filetime >> 32) as u32 };
    let mut utc = SYSTEMTIME::default();
    let mut local = SYSTEMTIME::default();
    // SAFETY: plain conversions between stack values.
    unsafe {
        if FileTimeToSystemTime(&ft, &mut utc).is_err() || SystemTimeToTzSpecificLocalTime(None, &utc, &mut local).is_err()
        {
            return None;
        }
    }
    Some(LocalTime {
        year: local.wYear,
        month: local.wMonth,
        day: local.wDay,
        hour: local.wHour,
        minute: local.wMinute,
        weekday: local.wDayOfWeek,
    })
}

#[cfg(not(windows))]
pub fn local_time(_filetime: u64) -> Option<LocalTime> {
    None
}

/// Now, as a FILETIME.
#[cfg(windows)]
pub fn now_filetime() -> u64 {
    // SAFETY: plain query.
    let ft = unsafe { windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime() };
    (u64::from(ft.dwHighDateTime) << 32) | u64::from(ft.dwLowDateTime)
}

#[cfg(not(windows))]
pub fn now_filetime() -> u64 {
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(year: u16, month: u16, day: u16, hour: u16, minute: u16, weekday: u16) -> LocalTime {
        LocalTime { year, month, day, hour, minute, weekday }
    }

    #[test]
    fn says_today_yesterday_weekday_then_date() {
        // Thursday 26 November 2026, 4:00 PM.
        let now = at(2026, 11, 26, 16, 0, 4);
        assert_eq!(relative(&at(2026, 11, 26, 9, 26, 4), &now), "Today 9:26 AM");
        assert_eq!(relative(&at(2026, 11, 25, 0, 5, 3), &now), "Yesterday 12:05 AM");
        assert_eq!(relative(&at(2026, 11, 23, 22, 37, 1), &now), "Mon 10:37 PM");
        assert_eq!(relative(&at(2026, 11, 20, 12, 0, 5), &now), "Fri 12:00 PM");
        assert_eq!(relative(&at(2026, 11, 19, 12, 0, 4), &now), "Nov 19");
        assert_eq!(relative(&at(2025, 11, 12, 8, 0, 3), &now), "Nov 12, 2025");
    }

    #[test]
    fn crosses_month_and_year_ends() {
        let new_year = at(2027, 1, 1, 9, 0, 5);
        assert_eq!(relative(&at(2026, 12, 31, 23, 59, 4), &new_year), "Yesterday 11:59 PM");
        assert_eq!(relative(&at(2026, 12, 28, 10, 0, 1), &new_year), "Mon 10:00 AM");
    }

    #[test]
    fn future_dates_show_the_date() {
        let now = at(2026, 11, 26, 16, 0, 4);
        assert_eq!(relative(&at(2026, 12, 1, 9, 0, 2), &now), "Dec 1");
    }

    #[test]
    fn absolute_keeps_the_old_form() {
        assert_eq!(absolute(&at(2026, 11, 22, 15, 4, 0)), "11/22/2026 3:04 PM");
    }
}
