// time_utils.rs
use chrono::{Datelike, TimeZone, Timelike, Utc};
//use log::*;

/// Calculates whether a given time is in Daylight Saving Time (CEST).
/// Summer: Last Sunday on March 2:00 UTC to last Sunday on October 3:00 UTC
pub fn is_dst(year: i32, month: u32, day: u32, hour: u32) -> bool {
    // Last Sunday in March (start of CEST)
    let march_last_sunday_day = 31 - ((5 * year / 4 + 4) % 7);

    // Last Sunday in October (end of CEST)
    let october_last_sunday_day = 31 - ((5 * year / 4 + 1) % 7);

    match month {
        1 | 2 => false, // January, February: always CET
        3 => {
            // March: CEST from last Sunday 2:00 UTC
            if day < march_last_sunday_day as u32 {
                false
            } else if day > march_last_sunday_day as u32 {
                true
            } else {
                // On the changeover day: from 2:00 UTC
                hour >= 2
            }
        }
        4..=9 => true, // April to September: always CEST
        10 => {
            // October: CEST until last Sunday 3:00 UTC
            if day < october_last_sunday_day as u32 {
                true
            } else if day > october_last_sunday_day as u32 {
                false
            } else {
                // On the changeover day: until 3:00 UTC
                hour < 3
            }
        }
        11 | 12 => false, // November, December: always CET
        _ => false,
    }
}

/// Converts UTC time to Berlin time (CET/CEST)
pub fn utc_to_berlin(utc_timestamp: i64) -> (i32, u32, u32, u32, u32, u32) {
    let utc_time = Utc.timestamp_opt(utc_timestamp, 0).unwrap();

    let year = utc_time.year();
    let month = utc_time.month();
    let day = utc_time.day();
    let hour = utc_time.hour();

    // Check if daylight saving time is active
    let offset_hours = if is_dst(year, month, day, hour) {
        2 // CEST: UTC+2
    } else {
        1 // CET: UTC+1
    };

    // Add offset
    let local_timestamp = utc_timestamp + (offset_hours * 3600);
    let local_time = Utc.timestamp_opt(local_timestamp, 0).unwrap();

    (
        local_time.year(),
        local_time.month(),
        local_time.day(),
        local_time.hour(),
        local_time.minute(),
        local_time.second(),
    )
}

/// Formats the time as a string "HH:MM:SS"
pub fn format_time(hour: u32, minute: u32, second: u32) -> String {
    format!("{:02}:{:02}:{:02}", hour, minute, second)
}

/// Formats the date as a string "DD.MM.YYYY"
pub fn format_date(day: u32, month: u32, year: i32) -> String {
    format!("{:02}.{:02}.{}", day, month, year)
}

/// Returns the current time zone as a string
pub fn get_timezone_str(year: i32, month: u32, day: u32, hour: u32) -> &'static str {
    if is_dst(year, month, day, hour) {
        "CEST"
    } else {
        "CET"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dst_calculation() {
        // March 15, 2024, 10:00 UTC -> CET (before changeover)
        assert_eq!(is_dst(2024, 3, 15, 10), false);

        // March 31, 2024, 03:00 UTC -> CEST (after changeover)
        assert_eq!(is_dst(2024, 3, 31, 3), true);

        // July 15, 2024, 12:00 UTC -> CEST
        assert_eq!(is_dst(2024, 7, 15, 12), true);

        // October 27, 2024, 02:00 UTC -> CET (after changeover)
        assert_eq!(is_dst(2024, 10, 27, 4), false);

        // December 15, 2024, 18:00 UTC -> CET
        assert_eq!(is_dst(2024, 12, 15, 18), false);
    }
}
