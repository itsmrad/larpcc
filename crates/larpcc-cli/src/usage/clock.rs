use std::time::{SystemTime, UNIX_EPOCH};

pub(super) const MIN_RESET: i64 = 1_577_836_800;
pub(super) const MAX_RESET: i64 = 4_102_444_800;

pub(crate) fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_secs() as i64)
}

fn days_before_year(year: i64) -> i64 {
    let previous = year - 1;
    let leaps = |y| y / 4 - y / 100 + y / 400;
    (year - 1970) * 365 + leaps(previous) - leaps(1969)
}

fn months(year: i64) -> [i64; 12] {
    let february = 28 + days_before_year(year + 1) - days_before_year(year) - 365;
    [31, february, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
}

pub(crate) fn parse(input: &str) -> Option<i64> {
    let (time, suffix) = input.as_bytes().split_at_checked(19)?;
    if [time[4], time[7], time[10], time[13], time[16]] != *b"--T::" {
        return None;
    }
    match suffix {
        b"Z" => {}
        [b'.', digits @ .., b'Z']
            if !digits.is_empty() && digits.len() <= 9 && digits.iter().all(u8::is_ascii_digit) => {
        }
        _ => return None,
    }
    let number = |start, end| -> Option<i64> {
        let digits: &[u8] = &time[start..end];
        if !digits.iter().all(u8::is_ascii_digit) {
            return None;
        }
        std::str::from_utf8(digits).ok()?.parse().ok()
    };
    let year = number(0, 4)?;
    let month = number(5, 7)?;
    let day = number(8, 10)?;
    let hour = number(11, 13)?;
    let minute = number(14, 16)?;
    let second = number(17, 19)?;
    if !(1970..=9999).contains(&year)
        || !(1..=12).contains(&month)
        || hour >= 24
        || minute >= 60
        || second >= 60
    {
        return None;
    }
    let months = months(year);
    let month = month as usize - 1;
    if !(1..=months[month]).contains(&day) {
        return None;
    }
    let days = days_before_year(year) + months[..month].iter().sum::<i64>() + day - 1;
    Some(days * 86400 + hour * 3600 + minute * 60 + second)
}

pub(crate) fn format(timestamp: i64) -> String {
    let days = timestamp.div_euclid(86400);
    let seconds = timestamp.rem_euclid(86400);
    let mut year = 1970 + days / 365;
    while days_before_year(year) > days {
        year -= 1;
    }
    let mut remaining = days - days_before_year(year);
    let mut month = 1;
    for length in months(year) {
        if remaining < length {
            break;
        }
        remaining -= length;
        month += 1;
    }
    let day = remaining + 1;
    let hour = seconds / 3600;
    let minute = seconds / 60 % 60;
    let second = seconds % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}
