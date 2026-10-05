//! Machine settings: `settings_get` and `settings_update`
//! (`docs/spec/05-project-format.md` §5.9, `docs/spec/02-architecture.md`
//! §2.5.2).
//!
//! `settings.json` is kept by [`b2c_store::SettingsStore`]; this module maps
//! its storage types to the IPC types and back. `settings_update` changes only
//! the code style, Run on errors and the console: its request type rejects
//! every other key when it is decoded, and the selected toolchain changes only
//! through `toolchain_select`.

use b2c_ipc::dto::{
    BuildCacheSettings, CodeStyle, ConsoleSettings, CppStandard as DtoStandard, IndentWidth,
    NewProjectSettings, NoticeReason as DtoReason, OnErrors as DtoOnErrors, RunSettings,
    Settings as DtoSettings, SettingsGetResponse, SettingsNotice as DtoNotice, SettingsPatch as DtoPatch,
    SettingsUpdateResponse, ToolchainSettings,
};
use b2c_ipc::{IpcError, ToolchainId};
use b2c_ir::sast::CppStandard;
use b2c_store::settings::{
    CodeStylePatch, ConsolePatch, RunPatch, SETTINGS_FORMAT_VERSION, Settings, SettingsNotice, SettingsPatch,
};
use b2c_store::{NoticeReason, OnErrors};

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
        }),
        console: patch.console.map(|console| ConsolePatch {
            scrollback_lines: console.scrollback_lines,
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
    /// Run on errors and the console, saves it atomically and returns the new
    /// settings. A rejected update changes nothing.
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
    use b2c_ipc::dto::{CodeStylePatch as DtoStyle, ConsolePatch as DtoConsole, RunSettingsPatch};

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
                "run": {"onErrors": "disableRun"},
                "console": {"scrollbackLines": 10000},
                "toolchain": {"selectedId": null},
                "newProject": {"standard": "c++20"},
                "buildCache": {"maxBytes": 2_147_483_648_u64}
            })
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
            }),
            console: Some(DtoConsole {
                scrollback_lines: Some(5000),
            }),
        });
        assert_eq!(patch.code_style.and_then(|s| s.indent_width), Some(2));
        assert_eq!(patch.run.and_then(|r| r.on_errors), Some(OnErrors::ShowProblems));
        assert_eq!(patch.console.and_then(|c| c.scrollback_lines), Some(5000));
        assert_eq!(patch_from_dto(&DtoPatch::default()), SettingsPatch::default());
    }
}
