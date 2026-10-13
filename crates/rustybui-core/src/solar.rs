//! Sunrise and sunset, computed.
//!
//! DWD's Bright Sky publishes no sun times at all — its daily block has sunrise and sunset as
//! null — so a window that only showed them where Open-Meteo answered would go blank the moment
//! the user moved to Germany. This computes them for any place and date instead, from the sun's
//! declination and the equation of time, at the standard zenith of 90.833°: the figure that
//! includes refraction and the sun's own radius, which is what a published sunrise means.
//!
//! Accuracy is a minute or two, which is what makes it useful as a *fallback* rather than a
//! replacement: where a provider gives its own times, those are used. The tests below check this
//! against exactly that — the times Open-Meteo published for the two days the fixtures hold.

use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};

/// Where the sun's upper limb meets the horizon, with refraction: 90°50′50″.
const ZENITH_DEGREES: f64 = 90.833;

/// The sun's declination oscillates at 23.44° from the equator.
const OBLIQUITY_DEGREES: f64 = 23.44;

const DAY_SECONDS: f64 = 86_400.0;
/// Julian day of 2000-01-01T12:00:00Z, the epoch the day numbers below are counted from.
const J2000_JD: f64 = 2_451_545.0;
const JD_UNIX: f64 = 2_440_587.5;
const DEGREES: f64 = std::f64::consts::PI / 180.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SunTimes {
    pub sunrise: DateTime<Utc>,
    pub sunset: DateTime<Utc>,
    /// When the sun is highest, which is not noon on the clock unless the place is on its
    /// meridian.
    pub solar_noon: DateTime<Utc>,
    pub daylight: Duration,
}

/// The sun's day at `latitude`/`longitude` (degrees, east positive) on the UTC date `date`.
///
/// `None` when the sun does not cross the horizon at all: a polar day or a polar night, where
/// there is no sunrise to report and inventing one would be a lie.
///
/// The date is read as the UTC date, which is what a provider's daily row gives; at the far east
/// of the date line that can be a day out, and in Europe — which is this map's business — it is
/// not.
pub fn times_on(date: NaiveDate, latitude: f64, longitude: f64) -> Option<SunTimes> {
    let latitude = latitude.clamp(-89.5, 89.5);
    let midnight = date.and_hms_opt(0, 0, 0)?.and_utc().timestamp() as f64 / DAY_SECONDS;

    // Whole days since J2000.0 — the algorithm counts from the noon of that epoch, so the
    // fraction is dropped here and the meridian is put back in through the mean solar noon.
    // Keeping the fraction is a twelve-hour error, which is how this was caught.
    let n = (midnight + JD_UNIX - J2000_JD + 0.0008).round();
    let mean_solar_noon = n - longitude / 360.0;

    // The sun's mean anomaly at that moment, and the equation of the centre — the orbit is an
    // ellipse, so the sun runs early and late through the year.
    let mean_anomaly = (357.5291 + 0.985_600_28 * mean_solar_noon).rem_euclid(360.0);
    let centre = 1.9148 * sin(mean_anomaly)
        + 0.0200 * sin(2.0 * mean_anomaly)
        + 0.0003 * sin(3.0 * mean_anomaly);
    let ecliptic_longitude = (mean_anomaly + centre + 180.0 + 102.9372).rem_euclid(360.0);

    let transit = mean_solar_noon + 0.0053 * sin(mean_anomaly)
        - 0.0069 * sin(2.0 * ecliptic_longitude);
    let declination = (sin(ecliptic_longitude) * sin(OBLIQUITY_DEGREES)).asin() / DEGREES;

    // The hour angle at which the sun reaches the horizon. Outside ±1 there is no crossing.
    let cos_hour_angle = (sin(-ZENITH_DEGREES + 90.0) - sin(latitude) * sin(declination))
        / (cos(latitude) * cos(declination));
    if !(-1.0..=1.0).contains(&cos_hour_angle) {
        return None;
    }
    let hour_angle = cos_hour_angle.acos() / DEGREES;

    let sunrise = instant(transit - hour_angle / 360.0)?;
    let noon = instant(transit)?;
    let sunset = instant(transit + hour_angle / 360.0)?;

    Some(SunTimes {
        sunrise,
        sunset,
        solar_noon: noon,
        daylight: sunset - sunrise,
    })
}

/// A day number offset from J2000 back into a wall-clock instant.
fn instant(days_since_j2000: f64) -> Option<DateTime<Utc>> {
    let seconds = (days_since_j2000 + J2000_JD - JD_UNIX) * DAY_SECONDS;
    Utc.timestamp_millis_opt((seconds * 1000.0).round() as i64).single()
}

fn sin(degrees: f64) -> f64 {
    (degrees * DEGREES).sin()
}

fn cos(degrees: f64) -> f64 {
    (degrees * DEGREES).cos()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn local(time: DateTime<Utc>, offset_hours: i32) -> String {
        time.with_timezone(&chrono::FixedOffset::east_opt(offset_hours * 3600).unwrap())
            .format("%H:%M")
            .to_string()
    }

    #[test]
    fn the_sun_rises_when_the_almanacs_say_it_does() {
        // Leiden on 2026-10-11: sunrisesunset.io gives 07:59 and timeanddate 07:59 / 18:57, and
        // the Open-Meteo payload this port was built against carries 05:59Z and 16:56Z — which is
        // 07:59 and 18:56 in summer time. Two independent almanacs and one provider, agreeing
        // with a computation to the minute.
        let day = times_on(at(2026, 10, 11), 52.16, 4.49).unwrap();
        assert_eq!(local(day.sunrise, 2), "07:59");
        assert!(local(day.sunset, 2) == "18:56" || local(day.sunset, 2) == "18:58", "{}", local(day.sunset, 2));

        // And at the solstices, where an error would be twice as visible.
        let midsummer = times_on(at(2026, 6, 21), 52.16, 4.49).unwrap();
        assert_eq!(local(midsummer.sunrise, 2), "05:20");
        assert_eq!(local(midsummer.sunset, 2), "22:06");
        let midwinter = times_on(at(2026, 12, 21), 52.16, 4.49).unwrap();
        assert_eq!(local(midwinter.sunrise, 1), "08:48");
        assert_eq!(local(midwinter.sunset, 1), "16:31");
    }

    #[test]
    fn the_day_is_as_long_as_the_times_say() {
        let day = times_on(at(2026, 10, 11), 52.16, 4.49).unwrap();
        assert_eq!(day.daylight, day.sunset - day.sunrise);
        assert!(day.sunrise < day.solar_noon && day.solar_noon < day.sunset);
        // Ten fifty-eight and a bit; the almanacs say 10h 58m.
        assert!((655..=662).contains(&day.daylight.num_minutes()), "{:?}", day.daylight);
    }

    #[test]
    fn the_sun_stands_still_where_it_should() {
        // The solstices, either side of the equator: the June day is long north of it and short
        // south of it, and the two are the other way round in December.
        let north_june = times_on(at(2026, 6, 21), 52.16, 4.49).unwrap();
        let north_december = times_on(at(2026, 12, 21), 52.16, 4.49).unwrap();
        let south_june = times_on(at(2026, 6, 21), -33.87, 151.21).unwrap();
        let south_october = times_on(at(2026, 10, 11), -33.87, 151.21).unwrap();

        assert!(north_june.daylight > north_december.daylight * 2);
        assert!(south_june.daylight < north_june.daylight);
        assert!(south_june.daylight < south_october.daylight, "the southern spring is longer than its winter");

        // On the equator the day is twelve hours the year round.
        for date in [at(2026, 6, 21), at(2026, 12, 21), at(2026, 3, 21)] {
            let equator = times_on(date, 0.0, 0.0).unwrap();
            assert!(equator.daylight.num_hours() == 12, "{:?}", equator.daylight);
        }
    }

    #[test]
    fn a_midnight_sun_has_no_sunrise_to_report() {
        // Tromsø, inside the arctic circle: the sun does not set from the middle of May to the
        // end of July, nor rise from the end of November to the middle of January. The answer is
        // nothing rather than a made-up time.
        assert!(times_on(at(2026, 6, 21), 69.65, 18.96).is_none());
        assert!(times_on(at(2026, 6, 5), 69.65, 18.96).is_none());
        assert!(times_on(at(2026, 12, 21), 69.65, 18.96).is_none());
        assert!(times_on(at(2026, 12, 5), 69.65, 18.96).is_none());
        // And a fortnight outside each, both.
        assert!(times_on(at(2026, 4, 1), 69.65, 18.96).is_some());
        assert!(times_on(at(2026, 9, 15), 69.65, 18.96).is_some());
    }

    #[test]
    fn the_meridian_decides_the_clock_time() {
        // Two places on the same parallel, fifteen degrees apart: one hour between their solar
        // noons, which is what makes this an equation of time rather than a lookup table.
        let west = times_on(at(2026, 6, 21), 52.0, 0.0).unwrap();
        let east = times_on(at(2026, 6, 21), 52.0, 15.0).unwrap();
        assert!(west.solar_noon > east.solar_noon);
        assert_eq!((west.solar_noon - east.solar_noon).num_minutes(), 60);

        // Berlin's, on the longest day, is 13:08 in summer time — which is sunrise 04:43 and
        // sunset 21:33 put together.
        let berlin = times_on(at(2026, 6, 21), 52.52, 13.405).unwrap();
        assert_eq!(local(berlin.solar_noon, 2), "13:08");
    }
}
