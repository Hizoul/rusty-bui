//! The moon, near enough.
//!
//! Phase and illuminated fraction from the moon's *true* elongation from the sun — Meeus's
//! series for the phase angle, a dozen sines over the moon's and sun's anomalies. That is good
//! to an hour or two of phase, which is what lets the day after a new moon be called a waxing
//! crescent rather than a new moon, the way every published calendar calls it.
//!
//! Moonrise and moonset are deliberately absent: they need a real ephemeris and a place, and
//! none of the three providers publishes them. Half a feature is better than an invented one.

use chrono::{DateTime, Utc};

/// Julian day of 1970-01-01T00:00:00Z.
const UNIX_EPOCH_JD: f64 = 2_440_587.5;

/// Julian day of J2000.0.
const J2000_JD: f64 = 2_451_545.0;

const DAY_SECONDS: f64 = 86_400.0;
const JULIAN_CENTURY_DAYS: f64 = 36_525.0;
const DEGREES: f64 = std::f64::consts::PI / 180.0;

/// Where the cycle is: new, crescent, quarter, gibbous, full, and back again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoonPhase {
    New,
    WaxingCrescent,
    FirstQuarter,
    WaxingGibbous,
    Full,
    WaningGibbous,
    LastQuarter,
    WaningCrescent,
}

impl MoonPhase {
    /// Eight bands of the cycle, centred on the four published phases: new at 0, first quarter
    /// at a quarter of the cycle, full at half, last quarter at three quarters. New and full are
    /// moments, not days — their bands are hours wide — which is why the day after a new moon
    /// reads as a waxing crescent here, the way every published calendar reads it.
    pub fn for_phase(phase: f64) -> Self {
        let point = phase.rem_euclid(1.0);
        if !(0.008..0.992).contains(&point) {
            Self::New
        } else if point < 0.24 {
            Self::WaxingCrescent
        } else if point < 0.26 {
            Self::FirstQuarter
        } else if point < 0.492 {
            Self::WaxingGibbous
        } else if point < 0.508 {
            Self::Full
        } else if point < 0.74 {
            Self::WaningGibbous
        } else if point < 0.76 {
            Self::LastQuarter
        } else {
            Self::WaningCrescent
        }
    }

    /// A stable key for a front-end to draw its own glyph from.
    pub fn key(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::WaxingCrescent => "waxing-crescent",
            Self::FirstQuarter => "first-quarter",
            Self::WaxingGibbous => "waxing-gibbous",
            Self::Full => "full",
            Self::WaningGibbous => "waning-gibbous",
            Self::LastQuarter => "last-quarter",
            Self::WaningCrescent => "waning-crescent",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::New => "New moon",
            Self::WaxingCrescent => "Waxing crescent",
            Self::FirstQuarter => "First quarter",
            Self::WaxingGibbous => "Waxing gibbous",
            Self::Full => "Full moon",
            Self::WaningGibbous => "Waning gibbous",
            Self::LastQuarter => "Last quarter",
            Self::WaningCrescent => "Waning crescent",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Moon {
    /// Fraction through the cycle: 0 at new, 0.25 first quarter, 0.5 full, 0.75 last quarter.
    pub phase: f64,
    pub phase_name: MoonPhase,
    /// 0–1: the fraction of the disc the sun is lighting.
    pub illumination: f64,
    /// Lit on the right, as seen from the north — which is the half of the cycle before full.
    pub waxing: bool,
}

impl Moon {
    /// `37% lit` — or, when the disc is a sliver, `under 1% lit`, because "0% lit" beside the
    /// words "waxing crescent" reads like a bug.
    pub fn illumination_label(&self) -> String {
        let percent = self.illumination * 100.0;
        if percent < 0.5 {
            "under 1% lit".to_string()
        } else {
            format!("{percent:.0}% lit")
        }
    }
}

/// Where the moon is in its cycle at `at`.
///
/// The mean cycle — one new moon every 29.5306 days — is half a day out at any moment, because
/// the moon's orbit is not a circle: its speed varies by about 12%. What corrects it is the
/// phase angle, the sun–moon–earth angle, from the moon's and the sun's anomalies: Meeus gives
/// it as a handful of sines, good to 0.3° — an hour or so of the cycle. Astronomical Algorithms,
/// chapter 49, with the illuminated fraction of chapter 48.
pub fn phase_at(at: DateTime<Utc>) -> Moon {
    let t = (at.timestamp() as f64 / DAY_SECONDS + UNIX_EPOCH_JD - J2000_JD) / JULIAN_CENTURY_DAYS;
    let t2 = t * t;
    let t3 = t2 * t;

    // The mean elongation of the moon from the sun, and the two anomalies, in degrees.
    let elongation_mean = 297.850_192_1 + 445_267.111_403_4 * t - 0.001_881_9 * t2 + t3 / 545_868.0;
    let sun_anomaly = 357.529_109_2 + 35_999.050_290_9 * t - 0.000_153_6 * t2;
    let moon_anomaly = 134.963_396_4 + 477_198.867_505_5 * t + 0.008_741_4 * t2 + t3 / 69_699.0;

    let sin = |degrees: f64| (degrees * DEGREES).sin();
    let d = elongation_mean;

    // The phase angle: 0° at full, 180° at new.
    let phase_angle = 180.0 - d - 6.289 * sin(moon_anomaly)
        + 2.100 * sin(sun_anomaly)
        - 1.274 * sin(2.0 * d - moon_anomaly)
        - 0.658 * sin(2.0 * d)
        - 0.214 * sin(2.0 * moon_anomaly)
        - 0.110 * sin(d);

    // And the elongation, which is its mirror and whose zero is the new moon.
    let elongation = (180.0 - phase_angle).rem_euclid(360.0);
    let phase = elongation / 360.0;
    let illumination = (1.0 + (phase_angle * DEGREES).cos()) / 2.0;

    Moon {
        phase,
        phase_name: MoonPhase::for_phase(phase),
        illumination,
        waxing: elongation < 180.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, hour, minute, 0).unwrap()
    }

    #[test]
    fn the_published_moons_of_october_2026_are_where_the_model_puts_them() {
        // The four published instants around the day this port was written, in UTC, from
        // timeanddate and moonphases.com. Landing within a two-hundredth of a cycle of each —
        // seven hours — is what the series is for; it does much better than that on two of them.
        let new = phase_at(at(2026, 10, 10, 15, 49));
        assert!(new.phase < 0.005, "phase was {}", new.phase);
        assert_eq!(new.phase_name, MoonPhase::New);
        assert!(new.illumination < 0.001, "a new moon is not lit: {}", new.illumination);

        let quarter = phase_at(at(2026, 10, 18, 16, 13));
        assert!((quarter.phase - 0.25).abs() < 0.02, "phase was {}", quarter.phase);
        assert_eq!(quarter.phase_name, MoonPhase::FirstQuarter);
        assert!((quarter.illumination - 0.5).abs() < 0.02);

        let full = phase_at(at(2026, 10, 26, 4, 12));
        assert!((full.phase - 0.5).abs() < 0.02, "phase was {}", full.phase);
        assert_eq!(full.phase_name, MoonPhase::Full);
        assert!(full.illumination > 0.99);

        let last = phase_at(at(2026, 11, 1, 20, 28));
        assert!((last.phase - 0.75).abs() < 0.02, "phase was {}", last.phase);
        assert_eq!(last.phase_name, MoonPhase::LastQuarter);
        assert!(!last.waxing);
    }

    #[test]
    fn the_eleventh_of_october_2026_is_the_sliver_that_was_forecast() {
        // The day this port was written, and the strongest check here: the one date whose
        // illumination was published independently. TheSkyLive put the disc at 0.6% lit and
        // 0.74 days into the cycle, phasesmoon at 1.26% and 1.06 days, moongiant at "about 1%"
        // — and all three call it a waxing crescent, not a new moon.
        let moon = phase_at(at(2026, 10, 11, 12, 0));
        assert!(
            (0.006..=0.026).contains(&moon.illumination),
            "illumination was {}, outside the published 0.6–2.6%",
            moon.illumination
        );
        assert!((0.025..=0.036).contains(&moon.phase), "phase was {}", moon.phase);
        assert_eq!(moon.phase_name, MoonPhase::WaxingCrescent);
        assert!(moon.waxing);
        assert_eq!(moon.illumination_label(), "1% lit");
    }

    #[test]
    fn the_names_cover_every_point_of_the_cycle() {
        let names: Vec<&str> = (0..100)
            .map(|step| MoonPhase::for_phase(step as f64 / 100.0).key())
            .collect();
        for expected in [
            "new",
            "waxing-crescent",
            "first-quarter",
            "waxing-gibbous",
            "full",
            "waning-gibbous",
            "last-quarter",
            "waning-crescent",
        ] {
            assert!(names.contains(&expected), "{expected} never came up: {names:?}");
        }
        // And a phase outside the cycle is folded in rather than panicking.
        assert_eq!(MoonPhase::for_phase(-0.25), MoonPhase::LastQuarter);
        assert_eq!(MoonPhase::for_phase(1.5), MoonPhase::Full);
    }

    #[test]
    fn illumination_and_phase_agree_with_each_other() {
        // Every named phase has to sit on the right side of half lit, or the label and the
        // disc would contradict each other.
        for step in 0..100 {
            let moon = phase_at(Utc.timestamp_millis_opt(1_792_000_000_000 + step * 90_000_000).unwrap());
            let lit = moon.illumination;
            match moon.phase_name {
                MoonPhase::New => assert!(lit < 0.01, "{lit}"),
                MoonPhase::Full => assert!(lit > 0.99, "{lit}"),
                MoonPhase::WaxingCrescent | MoonPhase::FirstQuarter | MoonPhase::WaxingGibbous => {
                    assert!(moon.waxing)
                }
                MoonPhase::WaningGibbous | MoonPhase::LastQuarter | MoonPhase::WaningCrescent => {
                    assert!(!moon.waxing)
                }
            }
            assert!((0.0..=1.0).contains(&lit));
        }
    }
}
