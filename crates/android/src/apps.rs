//! Launcher activities, independent of the selected ADB transport.
use crate::{Android, CommandTransport, error};
use serde::{Deserialize, Serialize};
use unimation::{Effect, Receipt, Result};

/// An explicit Android package/activity pair. Validation happens at construction
/// and deserialization, before any command reaches the transport.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Component(String);
impl TryFrom<String> for Component {
    type Error = String;
    fn try_from(value: String) -> std::result::Result<Self, Self::Error> {
        let valid = value.split_once('/').is_some_and(|(package, activity)| {
            !package.is_empty()
                && !activity.is_empty()
                && package
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_')
                && activity
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'$'))
        });
        if !valid || value.len() > 1024 {
            return Err("expected a package/activity component".into());
        }
        Ok(Self(value))
    }
}
impl From<Component> for String {
    fn from(value: Component) -> Self {
        value.0
    }
}
#[derive(Debug, Serialize)]
pub struct LauncherActivities {
    pub components: Vec<Component>,
    /// Preserve platform diagnostics instead of silently discarding unparsed lines.
    pub raw: String,
}
#[derive(Debug, Serialize)]
pub struct LaunchReport {
    pub receipt: Receipt,
    pub output: String,
}
impl<D: CommandTransport> Android<D> {
    /// Lists launcher entries for Android's current user; not all installed services.
    pub fn launcher_activities(&mut self) -> Result<LauncherActivities> {
        let raw = String::from_utf8(self.run("cmd package query-activities --brief --components --user current -a android.intent.action.MAIN -c android.intent.category.LAUNCHER", 1024 * 1024)?)
            .map_err(|e| error("android_encoding", e, Effect::None))?;
        let components = raw
            .lines()
            .filter_map(|line| Component::try_from(line.trim().to_owned()).ok())
            .collect();
        Ok(LauncherActivities { components, raw })
    }
    pub fn launch(&mut self, component: &Component) -> Result<LaunchReport> {
        // Single quoting also protects nested Java class '$' characters.
        let output = String::from_utf8(self.run(&format!("am start -W --user current -a android.intent.action.MAIN -c android.intent.category.LAUNCHER -n '{}'", component.0), 64 * 1024)?)
            .map_err(|e| error("android_encoding", e, Effect::Unknown))?;
        if output.lines().any(|line| line.starts_with("Error:")) {
            return Err(error("android_launch", output, Effect::Unknown));
        }
        Ok(LaunchReport {
            receipt: Receipt {
                effect: Effect::Dispatched,
                route: "android.adb.activity_manager".into(),
            },
            output,
        })
    }
}
/// The native brief inventory is already line-oriented; JSON retains parsed and raw data.
pub fn write_inventory(
    mut writer: impl std::io::Write,
    report: &LauncherActivities,
    format: unimation::OutputFormat,
) -> std::io::Result<()> {
    if format == unimation::OutputFormat::Text {
        writer.write_all(report.raw.as_bytes())
    } else {
        unimation::output::write_value(writer, &serde_json::to_value(report)?, format)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn component_rejects_shell_syntax_and_ambiguous_pairs() {
        for invalid in [
            "",
            "a",
            "a/",
            "/B",
            "a/b/c",
            "a/b;reboot",
            "a/$(id)",
            "a/b'",
            "-a/b",
            "a/b\n",
        ] {
            assert!(
                Component::try_from(invalid.to_owned()).is_err(),
                "{invalid}"
            );
        }
        for valid in [
            "com.android.settings/.Settings",
            "org.app/org.app.Main$Nested",
        ] {
            assert!(Component::try_from(valid.to_owned()).is_ok());
        }
    }
}
