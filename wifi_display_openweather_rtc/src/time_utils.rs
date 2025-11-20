// time_utils.rs
use chrono::{Datelike, TimeZone, Timelike, Utc};
//use log::*;

/// Berechnet, ob ein bestimmter Zeitpunkt in der Sommerzeit (CEST) liegt
/// Sommerzeit: Letzter Sonntag im März 2:00 UTC bis letzter Sonntag im Oktober 3:00 UTC
pub fn is_dst(year: i32, month: u32, day: u32, hour: u32) -> bool {
    // Letzter Sonntag im März (Beginn CEST)
    let march_last_sunday_day = 31 - ((5 * year / 4 + 4) % 7);

    // Letzter Sonntag im Oktober (Ende CEST)
    let october_last_sunday_day = 31 - ((5 * year / 4 + 1) % 7);

    match month {
        1 | 2 => false, // Januar, Februar: immer CET
        3 => {
            // März: CEST ab letztem Sonntag 2:00 UTC
            if day < march_last_sunday_day as u32 {
                false
            } else if day > march_last_sunday_day as u32 {
                true
            } else {
                // Am Umstellungstag: ab 2:00 UTC
                hour >= 2
            }
        }
        4..=9 => true, // April bis September: immer CEST
        10 => {
            // Oktober: CEST bis letzter Sonntag 3:00 UTC
            if day < october_last_sunday_day as u32 {
                true
            } else if day > october_last_sunday_day as u32 {
                false
            } else {
                // Am Umstellungstag: bis 3:00 UTC
                hour < 3
            }
        }
        11 | 12 => false, // November, Dezember: immer CET
        _ => false,
    }
}

/// Konvertiert UTC-Zeit zu Berlin-Zeit (CET/CEST)
pub fn utc_to_berlin(utc_timestamp: i64) -> (i32, u32, u32, u32, u32, u32) {
    let utc_time = Utc.timestamp_opt(utc_timestamp, 0).unwrap();

    let year = utc_time.year();
    let month = utc_time.month();
    let day = utc_time.day();
    let hour = utc_time.hour();

    // Prüfe, ob Sommerzeit gilt
    let offset_hours = if is_dst(year, month, day, hour) {
        2 // CEST: UTC+2
    } else {
        1 // CET: UTC+1
    };

    // Addiere Offset
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

/// Formatiert die Zeit als String "HH:MM:SS"
pub fn format_time(hour: u32, minute: u32, second: u32) -> String {
    format!("{:02}:{:02}:{:02}", hour, minute, second)
}

/// Formatiert das Datum als String "DD.MM.YYYY"
pub fn format_date(day: u32, month: u32, year: i32) -> String {
    format!("{:02}.{:02}.{}", day, month, year)
}

/// Gibt die aktuelle Zeitzone zurück
pub fn get_timezone_str(year: i32, month: u32, day: u32, hour: u32) -> &'static str {
    if is_dst(year, month, day, hour) {
        "CEST"
    } else {
        "CET"
    }
}
