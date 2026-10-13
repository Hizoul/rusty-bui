//! One home for "how a number is written".
//!
//! These used to be scattered `format!`s, and they drifted: on a single line of the CLI's
//! forecast table the precipitation honoured the units preference while the temperature and the
//! wind speed beside it did not, and a rain rate printed as `0.40 in/h` was metric millimetres
//! wearing an imperial label. Everything a person reads goes through here, so a preference
//! cannot be half-applied.
//!
//! Metric is the app's default and its written form is the plain one: `14.2°`, `0.40 mm`,
//! `11.9 km/h`. Imperial spells its units out (`57.6°F`) — a bare degree sign already means
//! Celsius, and only where a reader could be in doubt does the extra letter earn its place.

/// `14.2°`, or `57.6°F`.
pub fn format_temperature(celsius: f64, imperial: bool) -> String {
    if imperial {
        format!("{:.1}°F", celsius * 9.0 / 5.0 + 32.0)
    } else {
        format!("{celsius:.1}°")
    }
}

/// `11.9 km/h`, or `7.4 mph`.
pub fn format_speed(kmh: f64, imperial: bool) -> String {
    if imperial {
        format!("{:.1} mph", kmh * 0.621_371)
    } else {
        format!("{kmh:.1} km/h")
    }
}

/// `0.40 mm`, or `0.016 in`.
pub fn format_millimetres(mm: f64, imperial: bool) -> String {
    if imperial {
        format!("{:.3} in", mm / 25.4)
    } else {
        format!("{mm:.2} mm")
    }
}

/// `0.40 mm/h`, or `0.016 in/h` — the number *and* the unit, because converting one without
/// the other is how `0.40 in/h` happened.
pub fn format_rate(mm_per_hour: f64, imperial: bool) -> String {
    if imperial {
        format!("{:.3} in/h", mm_per_hour / 25.4)
    } else {
        format!("{mm_per_hour:.2} mm/h")
    }
}

/// `1014 hPa`, or `29.94 inHg`.
pub fn format_pressure(hpa: f64, imperial: bool) -> String {
    if imperial {
        format!("{:.2} inHg", hpa * 0.029_53)
    } else {
        format!("{hpa:.0} hPa")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metric_is_the_plain_form() {
        assert_eq!(format_temperature(14.2, false), "14.2°");
        assert_eq!(format_speed(11.9, false), "11.9 km/h");
        assert_eq!(format_millimetres(0.4, false), "0.40 mm");
        assert_eq!(format_rate(0.4, false), "0.40 mm/h");
        assert_eq!(format_pressure(1014.0, false), "1014 hPa");
    }

    #[test]
    fn imperial_converts_and_says_so() {
        assert_eq!(format_temperature(14.2, true), "57.6°F");
        assert_eq!(format_temperature(0.0, true), "32.0°F");
        assert_eq!(format_temperature(-40.0, true), "-40.0°F");
        assert_eq!(format_speed(11.9, true), "7.4 mph");
        assert_eq!(format_millimetres(25.4, true), "1.000 in");
        assert_eq!(format_rate(0.4, true), "0.016 in/h");
        assert_eq!(format_pressure(1013.25, true), "29.92 inHg");
    }
}
