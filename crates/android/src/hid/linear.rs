//! Opt-in native HID positioning with temporarily disabled mouse acceleration.
//! This changes the current Android user's mouse settings, including other mice.
//! Android/OEM support is verified by motion, not assumed from a settings write.
//! Close explicitly for checked restoration; Drop is best effort, not crash recovery.
use super::{Button, Pointer};
use crate::{
    CommandTransport, LimitedOutput,
    cursor::{self, CursorObservation, DisplayGeometry, Point},
    error,
};
use actuate::{
    Effect, Result,
    motion::{MotionStyle, RelativeMotionPlan},
};
use std::time::Duration;

const ACCELERATION: &str = "mouse_pointer_acceleration_enabled";
const SPEED: &str = "pointer_speed";

fn command(t: &mut impl CommandTransport, cmd: &str) -> Result<String> {
    let mut out = LimitedOutput::new(4096);
    let mut err = LimitedOutput::new(4096);
    if t.execute(cmd, &mut out, &mut err)? != Some(0) {
        return Err(error(
            "mouse_settings",
            String::from_utf8_lossy(&err.bytes),
            Effect::Unknown,
        ));
    }
    String::from_utf8(out.bytes)
        .map(|s| s.trim().to_owned())
        .map_err(|e| error("mouse_settings", e, Effect::Unknown))
}
fn read(t: &mut impl CommandTransport, user: u32, key: &str) -> Result<Option<i32>> {
    let value = command(t, &format!("settings --user {user} get system {key}"))?;
    if value == "null" {
        return Ok(None);
    }
    let parsed = value
        .parse::<i32>()
        .map_err(|e| error("mouse_settings", e, Effect::None))?;
    if (key == ACCELERATION && !matches!(parsed, 0 | 1))
        || (key == SPEED && !(-7..=7).contains(&parsed))
    {
        return Err(error(
            "mouse_settings",
            "unsupported setting value",
            Effect::None,
        ));
    }
    Ok(Some(parsed))
}
fn write(t: &mut impl CommandTransport, user: u32, key: &str, value: Option<i32>) -> Result<()> {
    let cmd = match value {
        Some(value) => format!("settings --user {user} put system {key} {value}"),
        None => format!("settings --user {user} delete system {key}"),
    };
    command(t, &cmd)?;
    if read(t, user, key)? != value {
        return Err(error(
            "mouse_settings",
            "setting write did not persist",
            Effect::Unknown,
        ));
    }
    Ok(())
}
struct SettingsLease {
    user: u32,
    originals: [Option<i32>; 2],
    active: bool,
}
impl SettingsLease {
    fn acquire(t: &mut impl CommandTransport) -> Result<Self> {
        let user = command(t, "am get-current-user")?
            .parse::<u32>()
            .map_err(|e| error("mouse_settings", e, Effect::None))?;
        let mut lease = Self {
            user,
            originals: [read(t, user, ACCELERATION)?, read(t, user, SPEED)?],
            active: true,
        };
        if let Err(e) =
            write(t, user, ACCELERATION, Some(0)).and_then(|_| write(t, user, SPEED, Some(-7)))
        {
            if let Err(cleanup) = lease.restore(t) {
                return Err(error(
                    "mouse_settings_restore",
                    format!("{e}; restoration failed: {cleanup}"),
                    Effect::Unknown,
                ));
            }
            return Err(e);
        }
        Ok(lease)
    }
    fn check(&self, t: &mut impl CommandTransport) -> Result<()> {
        if !self.active
            || command(t, "am get-current-user")? != self.user.to_string()
            || read(t, self.user, ACCELERATION)? != Some(0)
            || read(t, self.user, SPEED)? != Some(-7)
        {
            return Err(error(
                "mouse_settings_changed",
                "mouse settings or active user changed; recalibration required",
                Effect::None,
            ));
        }
        Ok(())
    }
    fn restore(&mut self, t: &mut impl CommandTransport) -> Result<()> {
        if !self.active {
            return Ok(());
        }
        // Attempt both restores even if one fails. Never overwrite a newer user value.
        let mut failures = Vec::new();
        for (i, (key, temporary)) in [(ACCELERATION, 0), (SPEED, -7)].into_iter().enumerate() {
            let result = (|| {
                let current = read(t, self.user, key)?;
                if current == self.originals[i] {
                    return Ok(());
                }
                if current != Some(temporary) {
                    return Err(error(
                        "mouse_settings_conflict",
                        format!("{key} changed externally; left unchanged"),
                        Effect::Unknown,
                    ));
                }
                write(t, self.user, key, self.originals[i])
            })();
            if let Err(e) = result {
                failures.push(e.to_string());
            }
        }
        if failures.is_empty() {
            self.active = false;
            Ok(())
        } else {
            Err(error(
                "mouse_settings_restore",
                failures.join("; "),
                Effect::Unknown,
            ))
        }
    }
}

/// A calibrated, unaccelerated native HID pointer on one validated display.
/// Creation performs four small hover probes; allow room around the current
/// hotspot. No buttons may be held during calibration. Settings are user-wide.
pub struct LinearPointer<'p, 'connection> {
    pointer: &'p mut Pointer<'connection>,
    settings: SettingsLease,
    geometry: DisplayGeometry,
    gain: Point,
}
impl<'p, 'connection> LinearPointer<'p, 'connection> {
    pub fn open(pointer: &'p mut Pointer<'connection>) -> Result<Self> {
        if pointer.buttons != 0 {
            return Err(error(
                "hid_buttons_held",
                "release buttons before calibrating",
                Effect::None,
            ));
        }
        let initial = cursor::observe(pointer)?;
        let settings = SettingsLease::acquire(pointer)?;
        let mut result = Self {
            pointer,
            settings,
            geometry: initial.geometry.clone(),
            gain: Point { x: 0.0, y: 0.0 },
        };
        result.gain = match result.calibrate() {
            Ok(gain) => gain,
            Err(e) => {
                if let Err(cleanup) = result.close() {
                    return Err(error(
                        "hid_calibration_restore",
                        format!("{e}; {cleanup}"),
                        Effect::Unknown,
                    ));
                }
                return Err(e);
            }
        };
        Ok(result)
    }
    pub fn geometry(&self) -> &DisplayGeometry {
        &self.geometry
    }
    pub fn pixels_per_count(&self) -> Point {
        self.gain
    }
    fn observe(&mut self) -> Result<CursorObservation> {
        let o = cursor::observe(self.pointer)?;
        if o.geometry != self.geometry {
            return Err(error(
                "hid_geometry_changed",
                "display changed; recalibration required",
                Effect::Unknown,
            ));
        }
        Ok(o)
    }
    fn path(
        &mut self,
        dx: i32,
        dy: i32,
        duration: Duration,
        steps: usize,
        style: MotionStyle,
    ) -> Result<()> {
        self.pointer
            .move_smooth(&RelativeMotionPlan::new(dx, dy, duration, steps, style)?)?;
        self.pointer.connection.runtime.run(
            self.pointer.connection.timeout,
            Effect::Unknown,
            async {
                tokio::time::sleep(Duration::from_millis(50)).await;
                Ok(())
            },
        )
    }
    fn calibrate(&mut self) -> Result<Point> {
        self.settings.check(self.pointer)?;
        let start = self.observe()?.position;
        if start.x < 128.0
            || start.y < 128.0
            || start.x > self.geometry.width as f32 - 128.0
            || start.y > self.geometry.height as f32 - 128.0
        {
            return Err(error(
                "hid_calibration_space",
                "move the cursor at least 128 pixels inside the display before calibration",
                Effect::None,
            ));
        }
        let mut before = start;
        let mut gains = Vec::new();
        for (dx, dy, ms, steps) in [
            (64, 0, 400, 24),
            (-64, 0, 100, 6),
            (0, 64, 100, 6),
            (0, -64, 400, 24),
        ] {
            self.path(
                dx,
                dy,
                Duration::from_millis(ms),
                steps,
                MotionStyle::Straight,
            )?;
            let after = self.observe()?.position;
            let (distance, cross, counts) = if dx != 0 {
                (after.x - before.x, after.y - before.y, dx)
            } else {
                (after.y - before.y, after.x - before.x, dy)
            };
            if cross.abs() > 0.25 {
                return Err(error(
                    "hid_calibration",
                    "unexpected cross-axis movement",
                    Effect::Unknown,
                ));
            }
            gains.push(distance / counts as f32);
            before = after;
        }
        let gain = validate_gains(&gains)?;
        if (before.x - start.x).hypot(before.y - start.y) > 0.5 {
            return Err(error(
                "hid_calibration",
                "probe did not return to origin",
                Effect::Unknown,
            ));
        }
        self.settings.check(self.pointer)?;
        Ok(gain)
    }
    /// Smooth native movement, then endpoint verification. No click is sent on
    /// a missed endpoint. Tiny residual corrections use the same measured gain.
    pub fn move_to(
        &mut self,
        target: Point,
        duration: Duration,
        style: MotionStyle,
    ) -> Result<CursorObservation> {
        self.settings.check(self.pointer)?;
        if !target.x.is_finite()
            || !target.y.is_finite()
            || target.x < 0.0
            || target.y < 0.0
            || target.x >= self.geometry.width as f32
            || target.y >= self.geometry.height as f32
        {
            return Err(error(
                "hid_target",
                "target outside calibrated display",
                Effect::None,
            ));
        }
        if duration > Duration::from_secs(60) {
            return Err(error(
                "hid_duration",
                "duration exceeds 60 seconds",
                Effect::None,
            ));
        }
        let mut o = self.observe()?;
        for attempt in 0..3 {
            if (o.position.x - target.x).hypot(o.position.y - target.y) <= 0.75 {
                return Ok(o);
            }
            let dx = ((target.x - o.position.x) / self.gain.x).round() as i32;
            let dy = ((target.y - o.position.y) / self.gain.y).round() as i32;
            self.path(
                dx,
                dy,
                if attempt == 0 {
                    duration
                } else {
                    Duration::from_millis(80)
                },
                if attempt == 0 { 40 } else { 4 },
                if attempt == 0 {
                    style
                } else {
                    MotionStyle::Straight
                },
            )?;
            o = self.observe().map_err(after_motion)?;
            self.settings.check(self.pointer).map_err(after_motion)?;
        }
        if (o.position.x - target.x).hypot(o.position.y - target.y) <= 0.75 {
            Ok(o)
        } else {
            Err(error(
                "hid_target_unreached",
                "native pointer missed calibrated endpoint; no click dispatched",
                Effect::Unknown,
            ))
        }
    }
    pub fn click_at(&mut self, target: Point, button: Button) -> Result<CursorObservation> {
        let o = self.move_to(target, Duration::from_millis(500), MotionStyle::Curved)?;
        let press = self.pointer.button(button, true);
        let release = self.pointer.button(button, false);
        press?;
        release?;
        Ok(o)
    }
    /// Scroll counts remain wheel units, not pixels of application content.
    pub fn scroll_at(
        &mut self,
        target: Point,
        vertical: super::Delta,
        horizontal: super::Delta,
    ) -> Result<()> {
        self.move_to(target, Duration::from_millis(350), MotionStyle::Curved)?;
        self.pointer.scroll(vertical, horizontal)?;
        Ok(())
    }
    pub fn close(&mut self) -> Result<()> {
        self.settings.restore(self.pointer)
    }
}
impl Drop for LinearPointer<'_, '_> {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
impl CommandTransport for LinearPointer<'_, '_> {
    fn execute(
        &mut self,
        command: &str,
        stdout: &mut dyn std::io::Write,
        stderr: &mut dyn std::io::Write,
    ) -> Result<Option<u8>> {
        self.pointer.execute(command, stdout, stderr)
    }
}
fn after_motion(mut failure: actuate::NativeError) -> actuate::NativeError {
    failure.effect = Effect::Unknown;
    failure
}

fn validate_gains(g: &[f32]) -> Result<Point> {
    if g.len() != 4
        || g.iter().any(|v| !v.is_finite() || *v < 0.1 || *v > 1.0)
        || (g[0] - g[1]).abs() > g[0] * 0.02
        || (g[2] - g[3]).abs() > g[2] * 0.02
    {
        return Err(error(
            "hid_calibration",
            "mouse is not consistently linear at subpixel gain; acceleration setting may be unsupported",
            Effect::Unknown,
        ));
    }
    Ok(Point {
        x: (g[0] + g[1]) / 2.0,
        y: (g[2] + g[3]) / 2.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    struct Fake {
        values: [Option<i32>; 2],
        fail_speed: bool,
    }
    impl CommandTransport for Fake {
        fn execute(
            &mut self,
            cmd: &str,
            out: &mut dyn Write,
            _err: &mut dyn Write,
        ) -> Result<Option<u8>> {
            if cmd == "am get-current-user" {
                out.write_all(b"10\n").unwrap();
                return Ok(Some(0));
            }
            let a: Vec<_> = cmd.split_whitespace().collect();
            assert_eq!(&a[..3], &["settings", "--user", "10"]);
            let idx = match a[5] {
                ACCELERATION => 0,
                SPEED => 1,
                _ => panic!("unexpected key"),
            };
            match a[3] {
                "get" => writeln!(
                    out,
                    "{}",
                    self.values[idx]
                        .map(|v| v.to_string())
                        .unwrap_or("null".into())
                )
                .unwrap(),
                "put" => {
                    let value = a[6].parse().unwrap();
                    if idx == 1 && value == -7 && self.fail_speed {
                        return Ok(Some(1));
                    }
                    self.values[idx] = Some(value);
                }
                "delete" => self.values[idx] = None,
                _ => panic!("unexpected operation"),
            }
            Ok(Some(0))
        }
    }
    #[test]
    fn settings_restore_absence_and_original_speed() {
        let mut t = Fake {
            values: [None, Some(2)],
            fail_speed: false,
        };
        let mut lease = SettingsLease::acquire(&mut t).unwrap();
        assert_eq!(t.values, [Some(0), Some(-7)]);
        lease.check(&mut t).unwrap();
        lease.restore(&mut t).unwrap();
        assert_eq!(t.values, [None, Some(2)]);
        assert!(lease.check(&mut t).is_err());
    }
    #[test]
    fn partial_acquisition_restores_first_setting() {
        let mut t = Fake {
            values: [Some(1), None],
            fail_speed: true,
        };
        assert!(SettingsLease::acquire(&mut t).is_err());
        assert_eq!(t.values, [Some(1), None]);
    }
    #[test]
    fn external_setting_change_is_preserved_and_reported() {
        let mut t = Fake {
            values: [None, Some(0)],
            fail_speed: false,
        };
        let mut lease = SettingsLease::acquire(&mut t).unwrap();
        t.values[1] = Some(4);
        assert!(lease.check(&mut t).is_err());
        assert!(lease.restore(&mut t).is_err());
        assert_eq!(t.values, [None, Some(4)]);
    }
    #[test]
    fn calibration_rejects_acceleration_and_supports_distinct_axis_scales() {
        let g = validate_gains(&[0.3, 0.301, 0.5, 0.502]).unwrap();
        assert!(g.y > g.x);
        for bad in [
            [0.3, 0.5, 0.3, 0.3],
            [0.3, 0.3, 0.3, 0.6],
            [0.0; 4],
            [f32::NAN; 4],
            [2.0; 4],
            [-0.3; 4],
        ] {
            assert!(validate_gains(&bad).is_err());
        }
    }
}
