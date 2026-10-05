//! Machine-local settings: `settings_get` and `settings_update`.
//!
//! `settings_update` takes a strict partial patch of the keys the settings page
//! edits (code style, run behaviour, console). The other keys are changed only by
//! their own commands (`toolchain_select`) or not over IPC at all, so a patch that
//! names them is rejected.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::commands::IpcRequest;
use crate::dto::CppStandard;
use crate::error::{InvalidReason, IpcError};
use crate::ids::ToolchainId;
use crate::limits::SCROLLBACK_LINES;
use crate::macros::string_enum;
use crate::schema::{FieldSchema, FieldSpec, ObjectSchema};

/// The indentation of generated C++, in spaces.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(type = "2 | 4"))]
pub enum IndentWidth {
    /// Two spaces.
    Two,
    /// Four spaces (the default).
    #[default]
    Four,
}

impl IndentWidth {
    /// The allowed widths, as JSON numbers.
    pub const VALUES: &'static [i64] = &[2, 4];

    /// The width in spaces.
    pub const fn spaces(self) -> u8 {
        match self {
            Self::Two => 2,
            Self::Four => 4,
        }
    }

    /// The width for `spaces`, when it is an allowed width.
    pub const fn from_spaces(spaces: u8) -> Option<Self> {
        match spaces {
            2 => Some(Self::Two),
            4 => Some(Self::Four),
            _ => None,
        }
    }
}

impl Serialize for IndentWidth {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u8(self.spaces())
    }
}

impl<'de> Deserialize<'de> for IndentWidth {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let spaces = u8::deserialize(deserializer)?;
        Self::from_spaces(spaces)
            .ok_or_else(|| serde::de::Error::custom("invalid value: an indent width must be 2 or 4"))
    }
}

string_enum! {
    /// What the Run button does while the project has errors (the backend refuses
    /// to build either way).
    pub enum OnErrors {
        /// Run is disabled, with a tooltip that leads to the first error.
        DisableRun = "disableRun",
        /// Run stays enabled and opens Problems at the first error.
        ShowProblems = "showProblems",
    }
}

string_enum! {
    /// What happened to a settings key when the settings were read.
    pub enum NoticeReason {
        /// The value was invalid and was reset to its default.
        InvalidValue = "invalidValue",
        /// The file could not be read and the defaults are in use.
        CorruptFile = "corruptFile",
        /// The file was written by a newer version; only the known keys were read.
        NewerVersion = "newerVersion",
    }
}

/// The code style of generated C++.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct CodeStyle {
    /// The indentation.
    pub indent_width: IndentWidth,
}

/// Run behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct RunSettings {
    /// What Run does while the project has errors.
    pub on_errors: OnErrors,
}

/// The console.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ConsoleSettings {
    /// How many lines the console keeps, within
    /// [`SCROLLBACK_LINES`](crate::limits::SCROLLBACK_LINES).
    pub scrollback_lines: u32,
}

/// The selected toolchain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ToolchainSettings {
    /// The selected toolchain, or `null` for the first usable one in discovery
    /// order.
    pub selected_id: Option<ToolchainId>,
}

/// Defaults for new projects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct NewProjectSettings {
    /// The C++ standard of new projects.
    pub standard: CppStandard,
}

/// The build cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct BuildCacheSettings {
    /// The size above which the least recently used builds are evicted, in bytes.
    pub max_bytes: u64,
}

/// The machine-local settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// The settings format version (1).
    pub format_version: u32,
    /// The code style of generated C++.
    pub code_style: CodeStyle,
    /// Run behaviour.
    pub run: RunSettings,
    /// The console.
    pub console: ConsoleSettings,
    /// The selected toolchain.
    pub toolchain: ToolchainSettings,
    /// Defaults for new projects.
    pub new_project: NewProjectSettings,
    /// The build cache.
    pub build_cache: BuildCacheSettings,
}

/// A problem found when the settings were read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SettingsNotice {
    /// The dotted path of the key, for example `console.scrollbackLines`.
    pub key: String,
    /// What happened.
    pub reason: NoticeReason,
}

/// The response of `settings_get`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SettingsGetResponse {
    /// The settings in effect.
    pub settings: Settings,
    /// Problems found when they were read.
    pub notices: Vec<SettingsNotice>,
}

/// The response of `settings_update`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SettingsUpdateResponse {
    /// The settings after the update.
    pub settings: Settings,
}

/// A change to [`CodeStyle`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CodeStylePatch {
    /// The new indentation, when it changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub indent_width: Option<IndentWidth>,
}

/// A change to [`RunSettings`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RunSettingsPatch {
    /// The new behaviour, when it changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub on_errors: Option<OnErrors>,
}

/// A change to [`ConsoleSettings`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ConsolePatch {
    /// The new scrollback size, when it changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub scrollback_lines: Option<u32>,
}

/// The request of `settings_update`: the keys to change. Absent keys stay as they
/// are; `null` is not accepted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SettingsPatch {
    /// Code style changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub code_style: Option<CodeStylePatch>,
    /// Run behaviour changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub run: Option<RunSettingsPatch>,
    /// Console changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub console: Option<ConsolePatch>,
}

// `i64::from` cannot be called in a constant; u32 always fits in i64.
#[allow(clippy::cast_lossless)]
const SCROLLBACK: FieldSchema = FieldSchema::Int {
    min: *SCROLLBACK_LINES.start() as i64,
    max: *SCROLLBACK_LINES.end() as i64,
};

static CODE_STYLE_PATCH: ObjectSchema = ObjectSchema {
    fields: &[FieldSpec::optional(
        "indentWidth",
        FieldSchema::IntOneOf {
            values: IndentWidth::VALUES,
        },
    )],
};

static RUN_SETTINGS_PATCH: ObjectSchema = ObjectSchema {
    fields: &[FieldSpec::optional(
        "onErrors",
        FieldSchema::Enum {
            values: OnErrors::VALUES,
        },
    )],
};

static CONSOLE_PATCH: ObjectSchema = ObjectSchema {
    fields: &[FieldSpec::optional("scrollbackLines", SCROLLBACK)],
};

impl IpcRequest for SettingsPatch {
    const COMMAND: &'static str = "settings_update";

    fn schema() -> &'static ObjectSchema {
        static SCHEMA: ObjectSchema = ObjectSchema {
            fields: &[
                FieldSpec::optional("codeStyle", FieldSchema::Object(&CODE_STYLE_PATCH)),
                FieldSpec::optional("run", FieldSchema::Object(&RUN_SETTINGS_PATCH)),
                FieldSpec::optional("console", FieldSchema::Object(&CONSOLE_PATCH)),
            ],
        };
        &SCHEMA
    }

    fn sample() -> Self {
        Self {
            code_style: Some(CodeStylePatch {
                indent_width: Some(IndentWidth::Two),
            }),
            run: Some(RunSettingsPatch {
                on_errors: Some(OnErrors::ShowProblems),
            }),
            console: Some(ConsolePatch {
                scrollback_lines: Some(20_000),
            }),
        }
    }

    fn validate(&self) -> Result<(), IpcError> {
        if let Some(lines) = self.console.and_then(|c| c.scrollback_lines)
            && !SCROLLBACK_LINES.contains(&lines)
        {
            return Err(IpcError::invalid(
                InvalidReason::OutOfRange,
                Some("console.scrollbackLines"),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indent_width_is_a_number() {
        assert_eq!(serde_json::to_string(&IndentWidth::Two).unwrap(), "2");
        assert_eq!(
            serde_json::from_str::<IndentWidth>("4").unwrap(),
            IndentWidth::Four
        );
        for bad in ["3", "0", "-2", "\"4\"", "4.0", "256"] {
            assert!(serde_json::from_str::<IndentWidth>(bad).is_err(), "{bad}");
        }
        assert_eq!(IndentWidth::default(), IndentWidth::Four);
    }

    #[test]
    fn scrollback_is_rechecked() {
        let mut patch = SettingsPatch::sample();
        patch.validate().unwrap();
        patch.console = Some(ConsolePatch {
            scrollback_lines: Some(999),
        });
        assert_eq!(
            patch.validate().unwrap_err(),
            IpcError::invalid(InvalidReason::OutOfRange, Some("console.scrollbackLines"))
        );
        SettingsPatch::default().validate().unwrap();
        assert_eq!(serde_json::to_string(&SettingsPatch::default()).unwrap(), "{}");
    }
}
