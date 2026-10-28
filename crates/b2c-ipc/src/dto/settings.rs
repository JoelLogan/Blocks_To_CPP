//! Machine-local settings: `settings_get` and `settings_update`.
//!
//! `settings_update` takes a strict partial patch of the keys the settings page
//! edits (code style, run behaviour, console, appearance, the editor's mode)
//! and of the editor's layout (`ui`).
//! The other keys are changed only by their own commands (`toolchain_select`) or
//! not over IPC at all, so a patch that names them is rejected.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::commands::IpcRequest;
use crate::dto::CppStandard;
use crate::error::{InvalidReason, IpcError};
use crate::ids::ToolchainId;
use crate::limits::{BOTTOM_DOCK_HEIGHT, RIGHT_DOCK_WIDTH, SCROLLBACK_LINES, TOOLBOX_WIDTH};
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
    /// The app's colour scheme.
    pub enum Theme {
        /// The system's light or dark setting, followed while the app runs.
        System = "system",
        /// Always light.
        Light = "light",
        /// Always dark.
        Dark = "dark",
    }
}

string_enum! {
    /// How blocks read (`docs/spec/03-block-language.md` §3.5.2); both give
    /// the same code.
    pub enum BlockLabels {
        /// Plain words, such as `add (x) to [v]`.
        Friendly = "friendly",
        /// The C++ the block stands for, such as `v.push_back(x)`.
        Cpp = "cpp",
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
    /// Whether programs run in a terminal window of their own rather than
    /// the console (`docs/spec/07-toolchain-build-run.md` §7.6.2).
    pub external_terminal: bool,
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

/// The app's appearance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct AppearanceSettings {
    /// The colour scheme.
    pub theme: Theme,
}

/// The editor's mode; expert mode is C++ labels with advanced blocks shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct EditorSettings {
    /// The block labels.
    pub block_labels: BlockLabels,
    /// Whether the toolbox shows advanced categories and blocks.
    pub show_advanced: bool,
}

/// The layout of the editor (`docs/spec/04-user-interface.md` §4.1), as the
/// user last left it. An absent value was never set: the first-start size
/// applies.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct LayoutSettings {
    /// The width of the toolbox's flyout, within
    /// [`TOOLBOX_WIDTH`](crate::limits::TOOLBOX_WIDTH).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub toolbox_width: Option<u32>,
    /// The width of the C++ dock, within
    /// [`RIGHT_DOCK_WIDTH`](crate::limits::RIGHT_DOCK_WIDTH).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub right_dock_width: Option<u32>,
    /// The height of the bottom dock, within
    /// [`BOTTOM_DOCK_HEIGHT`](crate::limits::BOTTOM_DOCK_HEIGHT).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub bottom_dock_height: Option<u32>,
    /// Whether the C++ dock is hidden.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub right_collapsed: Option<bool>,
    /// Whether the bottom dock is hidden.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub bottom_collapsed: Option<bool>,
}

/// The window's own state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct UiSettings {
    /// The layout of the editor.
    pub layout: LayoutSettings,
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
    /// The app's appearance.
    pub appearance: AppearanceSettings,
    /// The editor's mode.
    pub editor: EditorSettings,
    /// The window's own state.
    pub ui: UiSettings,
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
    /// Whether to run programs in an external terminal, when it changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub external_terminal: Option<bool>,
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

/// A change to [`AppearanceSettings`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AppearancePatch {
    /// The new colour scheme, when it changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub theme: Option<Theme>,
}

/// A change to [`EditorSettings`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EditorPatch {
    /// The new block labels, when they change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub block_labels: Option<BlockLabels>,
    /// Whether advanced blocks are now shown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub show_advanced: Option<bool>,
}

/// A change to [`LayoutSettings`]: each value given replaces the stored one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct LayoutPatch {
    /// The new width of the toolbox's flyout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub toolbox_width: Option<u32>,
    /// The new width of the C++ dock.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub right_dock_width: Option<u32>,
    /// The new height of the bottom dock.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub bottom_dock_height: Option<u32>,
    /// Whether the C++ dock is now hidden.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub right_collapsed: Option<bool>,
    /// Whether the bottom dock is now hidden.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub bottom_collapsed: Option<bool>,
}

/// A change to [`UiSettings`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct UiPatch {
    /// Layout changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub layout: Option<LayoutPatch>,
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
    /// Appearance changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub appearance: Option<AppearancePatch>,
    /// Changes to the editor's mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub editor: Option<EditorPatch>,
    /// Changes to the window's own state (the editor's layout).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub ui: Option<UiPatch>,
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
    fields: &[
        FieldSpec::optional(
            "onErrors",
            FieldSchema::Enum {
                values: OnErrors::VALUES,
            },
        ),
        FieldSpec::optional("externalTerminal", FieldSchema::Bool),
    ],
};

static CONSOLE_PATCH: ObjectSchema = ObjectSchema {
    fields: &[FieldSpec::optional("scrollbackLines", SCROLLBACK)],
};

static APPEARANCE_PATCH: ObjectSchema = ObjectSchema {
    fields: &[FieldSpec::optional(
        "theme",
        FieldSchema::Enum {
            values: Theme::VALUES,
        },
    )],
};

static EDITOR_PATCH: ObjectSchema = ObjectSchema {
    fields: &[
        FieldSpec::optional(
            "blockLabels",
            FieldSchema::Enum {
                values: BlockLabels::VALUES,
            },
        ),
        FieldSpec::optional("showAdvanced", FieldSchema::Bool),
    ],
};

/// A whole number of pixels in `range`, as a schema.
// `i64::from` cannot be called in a constant; u32 always fits in i64.
#[allow(clippy::cast_lossless)]
const fn pixels(range: &std::ops::RangeInclusive<u32>) -> FieldSchema {
    FieldSchema::Int {
        min: *range.start() as i64,
        max: *range.end() as i64,
    }
}

static LAYOUT_PATCH: ObjectSchema = ObjectSchema {
    fields: &[
        FieldSpec::optional("toolboxWidth", pixels(&TOOLBOX_WIDTH)),
        FieldSpec::optional("rightDockWidth", pixels(&RIGHT_DOCK_WIDTH)),
        FieldSpec::optional("bottomDockHeight", pixels(&BOTTOM_DOCK_HEIGHT)),
        FieldSpec::optional("rightCollapsed", FieldSchema::Bool),
        FieldSpec::optional("bottomCollapsed", FieldSchema::Bool),
    ],
};

static UI_PATCH: ObjectSchema = ObjectSchema {
    fields: &[FieldSpec::optional("layout", FieldSchema::Object(&LAYOUT_PATCH))],
};

impl IpcRequest for SettingsPatch {
    const COMMAND: &'static str = "settings_update";

    fn schema() -> &'static ObjectSchema {
        static SCHEMA: ObjectSchema = ObjectSchema {
            fields: &[
                FieldSpec::optional("codeStyle", FieldSchema::Object(&CODE_STYLE_PATCH)),
                FieldSpec::optional("run", FieldSchema::Object(&RUN_SETTINGS_PATCH)),
                FieldSpec::optional("console", FieldSchema::Object(&CONSOLE_PATCH)),
                FieldSpec::optional("appearance", FieldSchema::Object(&APPEARANCE_PATCH)),
                FieldSpec::optional("editor", FieldSchema::Object(&EDITOR_PATCH)),
                FieldSpec::optional("ui", FieldSchema::Object(&UI_PATCH)),
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
                external_terminal: Some(true),
            }),
            console: Some(ConsolePatch {
                scrollback_lines: Some(20_000),
            }),
            appearance: Some(AppearancePatch {
                theme: Some(Theme::Dark),
            }),
            editor: Some(EditorPatch {
                block_labels: Some(BlockLabels::Cpp),
                show_advanced: Some(true),
            }),
            ui: Some(UiPatch {
                layout: Some(LayoutPatch {
                    toolbox_width: Some(360),
                    right_dock_width: Some(420),
                    bottom_dock_height: Some(240),
                    right_collapsed: Some(false),
                    bottom_collapsed: Some(true),
                }),
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
        let layout = self.ui.and_then(|ui| ui.layout).unwrap_or_default();
        for (value, range, key) in [
            (layout.toolbox_width, &TOOLBOX_WIDTH, "ui.layout.toolboxWidth"),
            (
                layout.right_dock_width,
                &RIGHT_DOCK_WIDTH,
                "ui.layout.rightDockWidth",
            ),
            (
                layout.bottom_dock_height,
                &BOTTOM_DOCK_HEIGHT,
                "ui.layout.bottomDockHeight",
            ),
        ] {
            if value.is_some_and(|pixels| !range.contains(&pixels)) {
                return Err(IpcError::invalid(InvalidReason::OutOfRange, Some(key)));
            }
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

    #[test]
    fn layout_sizes_are_rechecked() {
        let with = |layout: LayoutPatch| SettingsPatch {
            ui: Some(UiPatch { layout: Some(layout) }),
            ..SettingsPatch::default()
        };
        for (layout, key) in [
            (
                LayoutPatch {
                    toolbox_width: Some(159),
                    ..LayoutPatch::default()
                },
                "ui.layout.toolboxWidth",
            ),
            (
                LayoutPatch {
                    right_dock_width: Some(961),
                    ..LayoutPatch::default()
                },
                "ui.layout.rightDockWidth",
            ),
            (
                LayoutPatch {
                    bottom_dock_height: Some(95),
                    ..LayoutPatch::default()
                },
                "ui.layout.bottomDockHeight",
            ),
        ] {
            assert_eq!(
                with(layout).validate().unwrap_err(),
                IpcError::invalid(InvalidReason::OutOfRange, Some(key))
            );
        }
        with(LayoutPatch {
            toolbox_width: Some(1200),
            right_dock_width: Some(200),
            bottom_dock_height: Some(720),
            right_collapsed: Some(true),
            bottom_collapsed: Some(false),
        })
        .validate()
        .unwrap();
    }
}
