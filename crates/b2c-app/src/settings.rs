//! Machine settings: `settings_get` and `settings_update`
//! (`docs/spec/05-project-format.md` §5.9, `docs/spec/02-architecture.md`
//! §2.5.2).
//!
//! `settings.json` is kept by [`b2c_store::SettingsStore`]; this module maps
//! its storage types to the IPC types and back. `settings_update` changes only
//! the code style, Run on errors, the console, the appearance, the editor's
//! mode and the editor's layout (`ui.layout`): its request type rejects every other key when it is decoded,
//! and the selected toolchain changes only through `toolchain_select`.

use b2c_ipc::dto::{
    AppearanceSettings as DtoAppearance, BlockLabels as DtoLabels, BuildCacheSettings, CodeStyle,
    ConsoleSettings, CppStandard as DtoStandard, EditorSettings as DtoEditor, IndentWidth,
    LayoutSettings as DtoLayout, NewProjectSettings, NoticeReason as DtoReason, OnErrors as DtoOnErrors,
    RunSettings, Settings as DtoSettings, SettingsGetResponse, SettingsNotice as DtoNotice,
    SettingsPatch as DtoPatch, SettingsUpdateResponse, Theme as DtoTheme, ToolchainSettings,
    UiSettings as DtoUi,
};
use b2c_ipc::{IpcError, ToolchainId};
use b2c_ir::sast::CppStandard;
use b2c_store::settings::{
    AppearancePatch, CodeStylePatch, ConsolePatch, EditorPatch, LayoutPatch, RunPatch,
    SETTINGS_FORMAT_VERSION, Settings, SettingsNotice, SettingsPatch, UiPatch,
};
use b2c_store::{BlockLabels, NoticeReason, OnErrors, Theme};

use crate::backend::{Backend, command_span};
use crate::errors::store_error;

/// The IPC form of a C++ standard.
pub(crate) fn standard_to_dto(standard: CppStandard) -> DtoStandard {
    match standard {
        CppStandard::Cpp17 => DtoStandard::Cpp17,
        CppStandard::Cpp20 => DtoStandard::Cpp20,
        CppStandard::Cpp23 => DtoStandard::Cpp23,
        CppStandard::Cpp26 => DtoStandard::Cpp26,
    }
}

/// The IPC form of a colour scheme.
fn theme_to_dto(theme: Theme) -> DtoTheme {
    match theme {
        Theme::System => DtoTheme::System,
        Theme::Light => DtoTheme::Light,
        Theme::Dark => DtoTheme::Dark,
    }
}

/// The store's form of a colour scheme.
fn theme_from_dto(theme: DtoTheme) -> Theme {
    match theme {
        DtoTheme::System => Theme::System,
        DtoTheme::Light => Theme::Light,
        DtoTheme::Dark => Theme::Dark,
    }
}

/// The IPC form of the block labels.
fn labels_to_dto(labels: BlockLabels) -> DtoLabels {
    match labels {
        BlockLabels::Friendly => DtoLabels::Friendly,
        BlockLabels::Cpp => DtoLabels::Cpp,
    }
}

/// The store's form of the block labels.
fn labels_from_dto(labels: DtoLabels) -> BlockLabels {
    match labels {
        DtoLabels::Friendly => BlockLabels::Friendly,
        DtoLabels::Cpp => BlockLabels::Cpp,
    }
}

/// The IPC form of the settings.
pub(crate) fn settings_to_dto(settings: &Settings) -> DtoSettings {
    DtoSettings {
        format_version: u32::try_from(SETTINGS_FORMAT_VERSION).unwrap_or(1),
        code_style: CodeStyle {
            indent_width: IndentWidth::from_spaces(settings.code_style.indent_width).unwrap_or_default(),
        },
        run: RunSettings {
            on_errors: match settings.run.on_errors {
                OnErrors::DisableRun => DtoOnErrors::DisableRun,
                OnErrors::ShowProblems => DtoOnErrors::ShowProblems,
            },
            external_terminal: settings.run.external_terminal,
        },
        console: ConsoleSettings {
            scrollback_lines: settings.console.scrollback_lines,
        },
        toolchain: ToolchainSettings {
            selected_id: settings
                .toolchain
                .selected_id
                .as_deref()
                .and_then(|id| ToolchainId::parse(id).ok()),
        },
        new_project: NewProjectSettings {
            standard: standard_to_dto(settings.new_project.standard),
        },
        build_cache: BuildCacheSettings {
            max_bytes: settings.build_cache.max_bytes,
        },
        appearance: DtoAppearance {
            theme: theme_to_dto(settings.appearance.theme),
        },
        editor: DtoEditor {
            block_labels: labels_to_dto(settings.editor.block_labels),
            show_advanced: settings.editor.show_advanced,
        },
        ui: DtoUi {
            layout: DtoLayout {
                toolbox_width: settings.ui.layout.toolbox_width,
                right_dock_width: settings.ui.layout.right_dock_width,
                bottom_dock_height: settings.ui.layout.bottom_dock_height,
                right_collapsed: settings.ui.layout.right_collapsed,
                bottom_collapsed: settings.ui.layout.bottom_collapsed,
            },
        },
    }
}

/// The IPC form of the notices from loading the settings.
pub(crate) fn notices_to_dto(notices: &[SettingsNotice]) -> Vec<DtoNotice> {
    notices
        .iter()
        .map(|notice| DtoNotice {
            key: notice.key.clone(),
            reason: match notice.reason {
                NoticeReason::InvalidValue => DtoReason::InvalidValue,
                NoticeReason::CorruptFile => DtoReason::CorruptFile,
                NoticeReason::NewerVersion => DtoReason::NewerVersion,
            },
        })
        .collect()
}

/// The store's form of a settings patch.
pub(crate) fn patch_from_dto(patch: &DtoPatch) -> SettingsPatch {
    SettingsPatch {
        code_style: patch.code_style.map(|style| CodeStylePatch {
            indent_width: style.indent_width.map(IndentWidth::spaces),
        }),
        run: patch.run.map(|run| RunPatch {
            on_errors: run.on_errors.map(|choice| match choice {
                DtoOnErrors::DisableRun => OnErrors::DisableRun,
                DtoOnErrors::ShowProblems => OnErrors::ShowProblems,
            }),
            external_terminal: run.external_terminal,
        }),
        console: patch.console.map(|console| ConsolePatch {
            scrollback_lines: console.scrollback_lines,
        }),
        appearance: patch.appearance.map(|appearance| AppearancePatch {
            theme: appearance.theme.map(theme_from_dto),
        }),
        editor: patch.editor.map(|editor| EditorPatch {
            block_labels: editor.block_labels.map(labels_from_dto),
            show_advanced: editor.show_advanced,
        }),
        ui: patch.ui.map(|ui| UiPatch {
            layout: ui.layout.map(|layout| LayoutPatch {
                toolbox_width: layout.toolbox_width,
                right_dock_width: layout.right_dock_width,
                bottom_dock_height: layout.bottom_dock_height,
                right_collapsed: layout.right_collapsed,
                bottom_collapsed: layout.bottom_collapsed,
            }),
        }),
    }
}

// Commands take their request by value, as the adapter decodes it, so every
// command method has the same shape whether or not it keeps the request.
#[allow(clippy::needless_pass_by_value)]
impl Backend {
    /// `settings_get`: the settings in effect, with defaults filled in, and the
    /// notices from loading them (values reset, a corrupt or newer file).
    ///
    /// # Errors
    /// None today; the `Result` keeps the command shape.
    pub fn settings_get(&self) -> Result<SettingsGetResponse, IpcError> {
        let _span = command_span("settings_get");
        Ok(SettingsGetResponse {
            settings: settings_to_dto(&self.settings.get()),
            notices: self.settings_notices.clone(),
        })
    }

    /// `settings_update`: applies a strict partial update of the code style,
    /// Run on errors, the console and the editor's layout, saves it atomically
    /// and returns the new settings. A rejected update changes nothing.
    ///
    /// # Errors
    /// [`IpcError::InvalidRequest`] for a value out of range (also checked
    /// when the request is decoded); [`IpcError::Io`] when the file cannot be
    /// written (the settings stay as they were).
    pub fn settings_update(&self, patch: DtoPatch) -> Result<SettingsUpdateResponse, IpcError> {
        let _span = command_span("settings_update");
        b2c_ipc::IpcRequest::validate(&patch)?;
        let settings = self
            .settings
            .update(&patch_from_dto(&patch))
            .map_err(|error| match error {
                b2c_store::StoreError::Invalid(_) => {
                    IpcError::invalid(b2c_ipc::InvalidReason::OutOfRange, None)
                }
                other => store_error("update the settings", &other),
            })?;
        Ok(SettingsUpdateResponse {
            settings: settings_to_dto(&settings),
        })
    }

    /// The selected toolchain, when the settings name a valid ID.
    pub(crate) fn selected_toolchain(&self) -> Option<ToolchainId> {
        self.settings
            .get()
            .toolchain
            .selected_id
            .and_then(|id| ToolchainId::parse(&id).ok())
    }
}

#[cfg(test)]
mod tests {
    use b2c_ipc::dto::{
        AppearancePatch as DtoAppearancePatch, CodeStylePatch as DtoStyle, ConsolePatch as DtoConsole,
        EditorPatch as DtoEditorPatch, LayoutPatch as DtoLayoutPatch, RunSettingsPatch,
        UiPatch as DtoUiPatch,
    };
    use b2c_store::settings::LayoutSettings;

    use super::*;

    #[test]
    fn defaults_map_to_the_contract() {
        let dto = settings_to_dto(&Settings::default());
        let json = serde_json::to_value(&dto).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "formatVersion": 1,
                "codeStyle": {"indentWidth": 4},
                "run": {"onErrors": "disableRun", "externalTerminal": false},
                "console": {"scrollbackLines": 10000},
                "toolchain": {"selectedId": null},
                "newProject": {"standard": "c++20"},
                "buildCache": {"maxBytes": 2_147_483_648_u64},
                "appearance": {"theme": "system"},
                "editor": {"blockLabels": "friendly", "showAdvanced": false},
                "ui": {"layout": {}}
            })
        );
    }

    #[test]
    fn the_layout_maps_both_ways() {
        let mut settings = Settings::default();
        settings.ui.layout = LayoutSettings {
            toolbox_width: Some(360),
            right_dock_width: Some(420),
            bottom_dock_height: Some(240),
            right_collapsed: Some(true),
            bottom_collapsed: None,
        };
        assert_eq!(
            serde_json::to_value(settings_to_dto(&settings).ui).unwrap(),
            serde_json::json!({"layout": {
                "toolboxWidth": 360,
                "rightDockWidth": 420,
                "bottomDockHeight": 240,
                "rightCollapsed": true
            }})
        );
        let patch = patch_from_dto(&DtoPatch {
            ui: Some(DtoUiPatch {
                layout: Some(DtoLayoutPatch {
                    toolbox_width: Some(200),
                    right_dock_width: Some(300),
                    bottom_dock_height: Some(100),
                    right_collapsed: Some(false),
                    bottom_collapsed: Some(true),
                }),
            }),
            ..DtoPatch::default()
        });
        let layout = patch.ui.and_then(|ui| ui.layout).unwrap();
        assert_eq!(
            (
                layout.toolbox_width,
                layout.right_dock_width,
                layout.bottom_dock_height,
                layout.right_collapsed,
                layout.bottom_collapsed
            ),
            (Some(200), Some(300), Some(100), Some(false), Some(true))
        );
        // The store and the contract allow the same sizes.
        assert_eq!(b2c_store::settings::TOOLBOX_WIDTH, b2c_ipc::limits::TOOLBOX_WIDTH);
        assert_eq!(
            b2c_store::settings::RIGHT_DOCK_WIDTH,
            b2c_ipc::limits::RIGHT_DOCK_WIDTH
        );
        assert_eq!(
            b2c_store::settings::BOTTOM_DOCK_HEIGHT,
            b2c_ipc::limits::BOTTOM_DOCK_HEIGHT
        );
    }

    #[test]
    fn the_appearance_and_the_editor_mode_map_both_ways() {
        for theme in DtoTheme::ALL {
            assert_eq!(theme_to_dto(theme_from_dto(*theme)), *theme);
        }
        for labels in DtoLabels::ALL {
            assert_eq!(labels_to_dto(labels_from_dto(*labels)), *labels);
        }
        let mut settings = Settings::default();
        settings.appearance.theme = Theme::Dark;
        settings.editor.block_labels = BlockLabels::Cpp;
        settings.editor.show_advanced = true;
        let json = serde_json::to_value(settings_to_dto(&settings)).unwrap();
        assert_eq!(json["appearance"], serde_json::json!({"theme": "dark"}));
        assert_eq!(
            json["editor"],
            serde_json::json!({"blockLabels": "cpp", "showAdvanced": true})
        );
    }

    #[test]
    fn every_value_maps() {
        let mut settings = Settings::default();
        settings.code_style.indent_width = 2;
        settings.run.on_errors = OnErrors::ShowProblems;
        settings.toolchain.selected_id = Some(String::from("tc_0123456789abcdef"));
        for (standard, dto) in [
            (CppStandard::Cpp17, DtoStandard::Cpp17),
            (CppStandard::Cpp20, DtoStandard::Cpp20),
            (CppStandard::Cpp23, DtoStandard::Cpp23),
            (CppStandard::Cpp26, DtoStandard::Cpp26),
        ] {
            settings.new_project.standard = standard;
            let mapped = settings_to_dto(&settings);
            assert_eq!(mapped.new_project.standard, dto);
            assert_eq!(mapped.code_style.indent_width, IndentWidth::Two);
            assert_eq!(mapped.run.on_errors, DtoOnErrors::ShowProblems);
            assert_eq!(
                mapped.toolchain.selected_id.as_ref().map(ToolchainId::as_str),
                Some("tc_0123456789abcdef")
            );
        }
        // A selection that is not an ID (impossible from the store) reads as none.
        settings.toolchain.selected_id = Some(String::from("nope"));
        assert_eq!(settings_to_dto(&settings).toolchain.selected_id, None);
    }

    #[test]
    fn notices_and_patches_map() {
        let notices = notices_to_dto(&[
            SettingsNotice {
                key: String::new(),
                reason: NoticeReason::CorruptFile,
            },
            SettingsNotice {
                key: String::from("console.scrollbackLines"),
                reason: NoticeReason::InvalidValue,
            },
            SettingsNotice {
                key: String::new(),
                reason: NoticeReason::NewerVersion,
            },
        ]);
        let reasons: Vec<DtoReason> = notices.iter().map(|n| n.reason).collect();
        assert_eq!(
            reasons,
            [
                DtoReason::CorruptFile,
                DtoReason::InvalidValue,
                DtoReason::NewerVersion
            ]
        );
        assert_eq!(notices[1].key, "console.scrollbackLines");

        let patch = patch_from_dto(&DtoPatch {
            code_style: Some(DtoStyle {
                indent_width: Some(IndentWidth::Two),
            }),
            run: Some(RunSettingsPatch {
                on_errors: Some(DtoOnErrors::ShowProblems),
                external_terminal: Some(true),
            }),
            console: Some(DtoConsole {
                scrollback_lines: Some(5000),
            }),
            appearance: Some(DtoAppearancePatch {
                theme: Some(DtoTheme::Light),
            }),
            editor: Some(DtoEditorPatch {
                block_labels: Some(DtoLabels::Cpp),
                show_advanced: None,
            }),
            ui: None,
        });
        assert_eq!(patch.code_style.and_then(|s| s.indent_width), Some(2));
        assert_eq!(patch.run.and_then(|r| r.on_errors), Some(OnErrors::ShowProblems));
        assert_eq!(patch.run.and_then(|r| r.external_terminal), Some(true));
        assert_eq!(patch.console.and_then(|c| c.scrollback_lines), Some(5000));
        assert_eq!(patch.appearance.and_then(|a| a.theme), Some(Theme::Light));
        assert_eq!(patch.editor.and_then(|e| e.block_labels), Some(BlockLabels::Cpp));
        assert_eq!(patch.editor.and_then(|e| e.show_advanced), None);
        assert_eq!(patch_from_dto(&DtoPatch::default()), SettingsPatch::default());
    }
}
