//! Sphere and atmosphere arithmetic.

use crate::grid::wrap_lon;

/// Radius of the sphere the GFS model uses, in metres.
pub const EARTH_RADIUS_M: f64 = 6_371_229.0;

/// Scale height used to turn a height above ground into a pressure, metres.
const SCALE_HEIGHT_M: f64 = 8_000.0;

/// A wind vector in Earth-centred coordinates, metres per second.
pub type Vec3 = [f64; 3];

/// Turns a wind `(u, v)` (metres per second east and north) at a point into
/// an Earth-centred vector. Winds at different points can be averaged in this
/// form, which their east and north components cannot: "north" points in
/// different directions at different places, and in opposite directions on
/// the two sides of a pole.
pub fn wind_vector(lon: f64, lat: f64, u: f64, v: f64) -> Vec3 {
    let (sl, cl) = lon.to_radians().sin_cos();
    let (sf, cf) = lat.to_radians().sin_cos();
    [-u * sl - v * sf * cl, u * cl - v * sf * sl, v * cf]
}

/// Moves a point with the velocity `wind` for `dt` seconds, staying on the
/// sphere. A negative `dt` moves it against the wind. Returns `(lon, lat)`
/// in degrees.
pub fn displace(lon: f64, lat: f64, wind: Vec3, dt: f64) -> (f64, f64) {
    let (sl, cl) = lon.to_radians().sin_cos();
    let (sf, cf) = lat.to_radians().sin_cos();
    let k = dt / EARTH_RADIUS_M;
    let x = cf * cl + wind[0] * k;
    let y = cf * sl + wind[1] * k;
    let z = sf + wind[2] * k;
    let lat2 = z.atan2(x.hypot(y)).to_degrees();
    (wrap_lon(y.atan2(x).to_degrees()), lat2)
}

/// Great-circle distance between two points in metres.
pub fn distance_m(lon1: f64, lat1: f64, lon2: f64, lat2: f64) -> f64 {
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let dl = (lon2 - lon1).to_radians();
    let a = ((p2 - p1) / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_M * a.sqrt().min(1.0).asin()
}

/// Pressure in pascals at `height_m` above ground with surface pressure `ps`.
pub fn pressure_above_ground(ps: f64, height_m: f64) -> f64 {
    ps * (-height_m / SCALE_HEIGHT_M).exp()
}

/// Altitude above sea level in metres of a pressure in pascals, by the
/// International Standard Atmosphere (troposphere branch).
pub fn standard_altitude_m(pressure_pa: f64) -> f64 {
    44_330.77 * (1.0 - (pressure_pa / 101_325.0).powf(0.190_263))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn advect(lon: f64, lat: f64, u: f64, v: f64, dt: f64) -> (f64, f64) {
        displace(lon, lat, wind_vector(lon, lat, u, v), dt)
    }

    #[test]
    fn displace_follows_the_wind_and_reverses() {
        let (lon, lat) = advect(13.2, 55.7, 10.0, 0.0, 3600.0);
        assert!(lon > 13.2 && (lat - 55.7).abs() < 0.01);
        let step = distance_m(13.2, 55.7, lon, lat);
        assert!((step - 36_000.0).abs() < 1.0, "{step}");
        let (lon, lat) = advect(13.2, 55.7, 0.0, 10.0, -3600.0);
        assert!((lon - 13.2).abs() < 1e-9 && lat < 55.7);
    }

    #[test]
    fn displace_crosses_the_pole() {
        let (lon, lat) = advect(20.0, 89.9, 0.0, 30.0, 3600.0);
        assert!((lon - 200.0).abs() < 1e-6, "{lon}");
        assert!(lat < 89.9 && lat > 89.0);
    }

    #[test]
    fn opposite_winds_across_the_pole_do_not_cancel() {
        let a = wind_vector(270.0, 89.0, 0.0, -30.0);
        let b = wind_vector(90.0, 89.0, 0.0, 30.0);
        let mean = [
            (a[0] + b[0]) / 2.0,
            (a[1] + b[1]) / 2.0,
            (a[2] + b[2]) / 2.0,
        ];
        let speed = (mean[0].powi(2) + mean[1].powi(2) + mean[2].powi(2)).sqrt();
        assert!(speed > 29.9, "{speed}");
    }

    #[test]
    fn standard_atmosphere_matches_reference_levels() {
        assert!(standard_altitude_m(101_325.0).abs() < 1.0);
        assert!((standard_altitude_m(85_000.0) - 1_457.0).abs() < 5.0);
        assert!((standard_altitude_m(50_000.0) - 5_574.0).abs() < 10.0);
    }
}
