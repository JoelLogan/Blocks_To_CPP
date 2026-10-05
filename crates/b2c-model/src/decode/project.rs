//! Decoding the document header and project settings (spec §5.3).

use std::collections::BTreeSet;

use b2c_ir::ProjectId;
use b2c_ir::sast::CppStandard;
use b2c_ir::text::Ident;

use super::{Decoder, Seg};
use crate::codes;
use crate::document::{
    BuildConfiguration, BuildSettings, Configurations, Define, DefineValue, Document, FormattingStyle,
    Generator, Language, Module, Optimization, PackRef, Project, ProjectOptions, RunSettings, Sanitizer,
    WarningLevel, WorkingDirectory,
};
use crate::json::Json;
use crate::limits::MAX_MODULES;
use crate::text_rules::quote;

/// Every C++ standard, in `serde` order.
pub(super) const STANDARDS: [CppStandard; 4] = [
    CppStandard::Cpp17,
    CppStandard::Cpp20,
    CppStandard::Cpp23,
    CppStandard::Cpp26,
];
/// Every formatting style.
pub(super) const FORMATTING_STYLES: [FormattingStyle; 2] = [FormattingStyle::Stream, FormattingStyle::Format];
/// Every optimisation level.
pub(super) const OPTIMIZATIONS: [Optimization; 4] = [
    Optimization::None,
    Optimization::Debug,
    Optimization::Speed,
    Optimization::Size,
];
/// Every sanitizer.
pub(super) const SANITIZERS: [Sanitizer; 2] = [Sanitizer::Address, Sanitizer::Undefined];
/// Every warning level.
pub(super) const WARNING_LEVELS: [WarningLevel; 3] =
    [WarningLevel::Minimal, WarningLevel::Helpful, WarningLevel::Strict];
/// Every working directory choice.
pub(super) const WORKING_DIRECTORIES: [WorkingDirectory; 2] =
    [WorkingDirectory::Project, WorkingDirectory::Sandbox];

/// Whether `id` is a valid library pack ID: `[a-z][a-z0-9_-]{0,63}`. Pack IDs
/// name folders and block namespaces, so they are as strict as module names.
pub(super) fn is_pack_id(id: &str) -> bool {
    let mut bytes = id.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_lowercase())
        && id.len() <= 64
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

/// Whether `version` looks like a SemVer requirement such as `^1.0`,
/// `>=1.2, <2` or `*`: 1 to 64 characters from digits, letters and
/// `. ^ ~ = < > * , + -` and spaces, with at least one that is not a space.
pub(super) fn is_version_requirement(version: &str) -> bool {
    (1..=64).contains(&version.len())
        && !version.trim().is_empty()
        && version.bytes().all(|b| {
            b.is_ascii_alphanumeric()
                || matches!(
                    b,
                    b'.' | b'^' | b'~' | b'=' | b'<' | b'>' | b'*' | b',' | b'+' | b'-' | b' '
                )
        })
}

/// Whether `name` is a valid library name: `[A-Za-z0-9_+.-]{1,64}`.
pub(super) fn is_library_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'+' | b'.' | b'-'))
}

impl<'a> Decoder<'a> {
    pub(super) fn document(&mut self, root: &'a Json) -> Option<Document> {
        let entries = self.object(
            root,
            &[
                "format",
                "formatVersion",
                "generator",
                "project",
                "modules",
                "x-ext",
            ],
        )?;
        let format = self.required(entries, "format", Self::string);
        let format_version = self.required(entries, "formatVersion", Self::u32);
        let generator = self.required(entries, "generator", Self::generator);
        let project = self.required(entries, "project", Self::project);
        let modules = self.required(entries, "modules", Self::modules);
        // "x-ext" is an object of tooling metadata (spec §5.6); what is
        // inside it is preserved without being interpreted.
        let ext = self.nullable(entries, "x-ext", |d, v| {
            if matches!(v, Json::Object(_)) {
                d.untyped(v, true)
            } else {
                d.wrong(v, "an object");
                None
            }
        });
        Some(Document {
            format: format?,
            format_version: format_version?,
            generator: generator?,
            project: project?,
            modules: modules?,
            ext: ext.ok()?,
        })
    }

    fn generator(&mut self, value: &'a Json) -> Option<Generator> {
        let entries = self.object(value, &["app", "catalog"])?;
        let app = self.required(entries, "app", Self::string);
        let catalog = self.required(entries, "catalog", Self::string);
        Some(Generator {
            app: app?,
            catalog: catalog?,
        })
    }

    fn project(&mut self, value: &'a Json) -> Option<Project> {
        let entries = self.object(
            value,
            &["id", "name", "description", "language", "options", "build", "run"],
        )?;
        let id = self.required(entries, "id", |d, v| d.id(v, ProjectId::new));
        let name = self.required(entries, "name", Self::string);
        let description = self.or_default(entries, "description", String::new, Self::string);
        let language = self.required(entries, "language", Self::language);
        let options = self.or_default(entries, "options", ProjectOptions::default, Self::options);
        let build = self.or_default(entries, "build", BuildSettings::default, Self::build);
        let run = self.or_default(entries, "run", RunSettings::default, Self::run_settings);
        Some(Project {
            id: id?,
            name: name?,
            description: description?,
            language: language?,
            options: options?,
            build: build?,
            run: run?,
        })
    }

    fn language(&mut self, value: &'a Json) -> Option<Language> {
        let entries = self.object(value, &["standard", "gnuExtensions"])?;
        let standard = self.required(entries, "standard", |d, v| d.choice(v, &STANDARDS));
        let gnu_extensions = self.or_default(entries, "gnuExtensions", || false, Self::bool);
        Some(Language {
            standard: standard?,
            gnu_extensions: gnu_extensions?,
        })
    }

    fn options(&mut self, value: &'a Json) -> Option<ProjectOptions> {
        let entries = self.object(
            value,
            &[
                "showAdvanced",
                "manualMemory",
                "preferPlainStd",
                "formattingStyle",
                "checkedIndexing",
            ],
        )?;
        let defaults = ProjectOptions::default();
        let show_advanced = self.or_default(entries, "showAdvanced", || defaults.show_advanced, Self::bool);
        let manual_memory = self.or_default(entries, "manualMemory", || defaults.manual_memory, Self::bool);
        let prefer_plain_std = self.or_default(
            entries,
            "preferPlainStd",
            || defaults.prefer_plain_std,
            Self::bool,
        );
        let formatting_style = self.or_default(
            entries,
            "formattingStyle",
            || defaults.formatting_style,
            |d, v| d.choice(v, &FORMATTING_STYLES),
        );
        let checked_indexing = self.or_default(
            entries,
            "checkedIndexing",
            || defaults.checked_indexing,
            Self::bool,
        );
        Some(ProjectOptions {
            show_advanced: show_advanced?,
            manual_memory: manual_memory?,
            prefer_plain_std: prefer_plain_std?,
            formatting_style: formatting_style?,
            checked_indexing: checked_indexing?,
        })
    }

    fn build(&mut self, value: &'a Json) -> Option<BuildSettings> {
        let entries = self.object(value, &["configurations", "defines", "libraries", "packs"])?;
        let configurations = self.or_default(
            entries,
            "configurations",
            Configurations::default,
            Self::configurations,
        );
        let defines = self.or_default(entries, "defines", Vec::new, Self::defines);
        let libraries = self.or_default(entries, "libraries", Vec::new, |d, v| d.list(v, Self::library));
        let packs = self.or_default(entries, "packs", Vec::new, Self::packs);
        Some(BuildSettings {
            configurations: configurations?,
            defines: defines?,
            libraries: libraries?,
            packs: packs?,
        })
    }

    fn configurations(&mut self, value: &'a Json) -> Option<Configurations> {
        let entries = self.object(value, &["debug", "release"])?;
        let debug = self.required(entries, "debug", Self::configuration);
        let release = self.required(entries, "release", Self::configuration);
        Some(Configurations {
            debug: debug?,
            release: release?,
        })
    }

    fn configuration(&mut self, value: &'a Json) -> Option<BuildConfiguration> {
        let entries = self.object(
            value,
            &[
                "optimization",
                "debugInfo",
                "sanitizers",
                "warnings",
                "warningsAsErrors",
                "hardening",
            ],
        )?;
        let optimization = self.required(entries, "optimization", |d, v| d.choice(v, &OPTIMIZATIONS));
        let debug_info = self.required(entries, "debugInfo", Self::bool);
        let sanitizers = self.or_default(entries, "sanitizers", Vec::new, |d, v| {
            d.list(v, |d, item| d.choice(item, &SANITIZERS))
        });
        let warnings = self.required(entries, "warnings", |d, v| d.choice(v, &WARNING_LEVELS));
        let warnings_as_errors = self.or_default(entries, "warningsAsErrors", || false, Self::bool);
        let hardening = self.required(entries, "hardening", Self::bool);
        Some(BuildConfiguration {
            optimization: optimization?,
            debug_info: debug_info?,
            sanitizers: sanitizers?,
            warnings: warnings?,
            warnings_as_errors: warnings_as_errors?,
            hardening: hardening?,
        })
    }

    fn defines(&mut self, value: &'a Json) -> Option<Vec<Define>> {
        let defines = self.list(value, Self::define)?;
        // Report a name used twice once, at its second use.
        let Json::Array(items) = value else {
            return Some(defines);
        };
        let mut seen = BTreeSet::new();
        for (index, item) in items.iter().enumerate() {
            if let Some(Json::String(name)) = item.get("name")
                && !seen.insert(&**name)
            {
                self.at(Seg::Index(index), |d| {
                    let message = format!(
                        "The preprocessor define {} is set more than once. Keep only one of them.",
                        quote(name)
                    );
                    d.report(codes::DUPLICATE_DEFINE, message);
                });
            }
        }
        Some(defines)
    }

    fn define(&mut self, value: &'a Json) -> Option<Define> {
        let entries = self.object(value, &["name", "value"])?;
        let name = self.required(entries, "name", |d, v| {
            let name = d.string(v)?;
            let valid = Ident::new(&name).and_then(|ident| ident.check_namespace_scope());
            match valid {
                Ok(()) => Some(name),
                Err(error) => {
                    let message = format!(
                        "The preprocessor define name {} cannot be used: {error}.",
                        quote(&name)
                    );
                    d.report(codes::BAD_DEFINE_NAME, message);
                    None
                }
            }
        });
        let value = self.required(entries, "value", Self::define_value);
        Some(Define {
            name: name?,
            value: value?,
        })
    }

    fn define_value(&mut self, value: &'a Json) -> Option<DefineValue> {
        const EXPECTED: &str = "one of {\"int\": 1}, {\"bool\": true} or {\"string\": \"text\"}";
        let Json::Object(entries) = value else {
            self.wrong(value, EXPECTED);
            return None;
        };
        let [(kind, inner)] = &**entries else {
            self.wrong(value, EXPECTED);
            self.reserved_keys_in(entries);
            return None;
        };
        match &**kind {
            "int" => self
                .at(Seg::Key(kind), |d| d.safe_int(inner))
                .map(DefineValue::Int),
            "bool" => self.at(Seg::Key(kind), |d| d.bool(inner)).map(DefineValue::Bool),
            "string" => self
                .at(Seg::Key(kind), |d| d.string(inner))
                .map(DefineValue::String),
            _ => {
                self.object(value, &["int", "bool", "string"]);
                None
            }
        }
    }

    fn library(&mut self, value: &'a Json) -> Option<String> {
        let name = self.string(value)?;
        if is_library_name(&name) {
            Some(name)
        } else {
            let message = format!(
                "The library name {} is not valid: use 1 to 64 letters (A–Z, a–z), digits and the characters _ + . and -.",
                quote(&name)
            );
            self.report(codes::BAD_LIBRARY_NAME, message);
            None
        }
    }

    fn packs(&mut self, value: &'a Json) -> Option<Vec<PackRef>> {
        let packs = self.list(value, Self::pack)?;
        // Report a pack listed twice once, at its second entry.
        let Json::Array(items) = value else {
            return Some(packs);
        };
        let mut seen = BTreeSet::new();
        for (index, item) in items.iter().enumerate() {
            if let Some(Json::String(id)) = item.get("id")
                && !seen.insert(&**id)
            {
                self.at(Seg::Index(index), |d| {
                    let message = format!(
                        "The library pack {} is listed more than once. Keep only one entry for it.",
                        quote(id)
                    );
                    d.report(codes::DUPLICATE_PACK, message);
                });
            }
        }
        Some(packs)
    }

    fn pack(&mut self, value: &'a Json) -> Option<PackRef> {
        let entries = self.object(value, &["id", "version"])?;
        let id = self.required(entries, "id", |d, v| {
            let id = d.string(v)?;
            if is_pack_id(&id) {
                return Some(id);
            }
            let message = format!(
                "The library pack ID {} is not valid: use 1 to 64 characters, a lower-case letter first, then lower-case letters, digits, _ or -.",
                quote(&id)
            );
            d.report(codes::BAD_PACK, message);
            None
        });
        let version = self.required(entries, "version", |d, v| {
            let version = d.string(v)?;
            if is_version_requirement(&version) {
                return Some(version);
            }
            let message = format!(
                "The library pack version {} is not a version requirement such as \"^1.0\" or \">=1.2, <2\".",
                quote(&version)
            );
            d.report(codes::BAD_PACK, message);
            None
        });
        Some(PackRef {
            id: id?,
            version: version?,
        })
    }

    fn run_settings(&mut self, value: &'a Json) -> Option<RunSettings> {
        let entries = self.object(value, &["args", "workingDirectory"])?;
        let args = self.or_default(entries, "args", Vec::new, |d, v| d.list(v, Self::string));
        let working_directory =
            self.or_default(entries, "workingDirectory", WorkingDirectory::default, |d, v| {
                d.choice(v, &WORKING_DIRECTORIES)
            });
        Some(RunSettings {
            args: args?,
            working_directory: working_directory?,
        })
    }

    fn modules(&mut self, value: &'a Json) -> Option<Vec<Module>> {
        let Json::Array(items) = value else {
            self.wrong(value, "a list");
            return None;
        };
        if items.is_empty() {
            self.report(
                codes::MODULE_COUNT,
                String::from("The project has no modules. It needs at least one (usually called \"main\")."),
            );
            return None;
        }
        if items.len() > MAX_MODULES {
            let message = format!(
                "The project has {} modules, but at most {MAX_MODULES} are allowed. Only the first {MAX_MODULES} were checked.",
                items.len()
            );
            self.report(codes::MODULE_COUNT, message);
        }
        let mut modules = Vec::with_capacity(items.len().min(MAX_MODULES));
        for (index, item) in items.iter().enumerate().take(MAX_MODULES) {
            if let Some(module) = self.at(Seg::Index(index), |d| d.module(item)) {
                modules.push(module);
            }
        }
        Some(modules)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::serde_name;

    // Fails to compile when a variant is added to a contract enum, so the
    // lists above stay complete.
    fn index_of_standard(value: CppStandard) -> usize {
        match value {
            CppStandard::Cpp17 => 0,
            CppStandard::Cpp20 => 1,
            CppStandard::Cpp23 => 2,
            CppStandard::Cpp26 => 3,
        }
    }

    fn index_of_style(value: FormattingStyle) -> usize {
        match value {
            FormattingStyle::Stream => 0,
            FormattingStyle::Format => 1,
        }
    }

    fn index_of_optimization(value: Optimization) -> usize {
        match value {
            Optimization::None => 0,
            Optimization::Debug => 1,
            Optimization::Speed => 2,
            Optimization::Size => 3,
        }
    }

    fn index_of_sanitizer(value: Sanitizer) -> usize {
        match value {
            Sanitizer::Address => 0,
            Sanitizer::Undefined => 1,
        }
    }

    fn index_of_warning(value: WarningLevel) -> usize {
        match value {
            WarningLevel::Minimal => 0,
            WarningLevel::Helpful => 1,
            WarningLevel::Strict => 2,
        }
    }

    fn index_of_directory(value: WorkingDirectory) -> usize {
        match value {
            WorkingDirectory::Project => 0,
            WorkingDirectory::Sandbox => 1,
        }
    }

    #[test]
    fn variant_lists_are_complete_and_ordered() {
        for (i, v) in STANDARDS.iter().enumerate() {
            assert_eq!(index_of_standard(*v), i);
        }
        for (i, v) in FORMATTING_STYLES.iter().enumerate() {
            assert_eq!(index_of_style(*v), i);
        }
        for (i, v) in OPTIMIZATIONS.iter().enumerate() {
            assert_eq!(index_of_optimization(*v), i);
        }
        for (i, v) in SANITIZERS.iter().enumerate() {
            assert_eq!(index_of_sanitizer(*v), i);
        }
        for (i, v) in WARNING_LEVELS.iter().enumerate() {
            assert_eq!(index_of_warning(*v), i);
        }
        for (i, v) in WORKING_DIRECTORIES.iter().enumerate() {
            assert_eq!(index_of_directory(*v), i);
        }
        assert_eq!(serde_name(&CppStandard::Cpp20).as_deref(), Some("c++20"));
        assert_eq!(serde_name(&Optimization::Size).as_deref(), Some("size"));
    }

    #[test]
    fn pack_ids_and_versions() {
        for good in ["std", "sfml", "my-pack_2", &format!("a{}", "b".repeat(63))] {
            assert!(is_pack_id(good), "{good}");
        }
        for bad in [
            "",
            "Std",
            "1x",
            "_x",
            "../evil",
            "a/b",
            "a.b",
            "é",
            &"a".repeat(65),
        ] {
            assert!(!is_pack_id(bad), "{bad}");
        }
        for good in ["^1.0", "1.2.3", "*", ">=1.2, <2", "~1.4", "=1.0.0-beta.1+build.5"] {
            assert!(is_version_requirement(good), "{good}");
        }
        for bad in [
            "",
            "   ",
            "1.0; rm -rf ~",
            "$(x)",
            "../1",
            "1\n2",
            &"1".repeat(65),
        ] {
            assert!(!is_version_requirement(bad), "{bad}");
        }
    }

    #[test]
    fn library_names() {
        for good in ["sfml-graphics", "a", "gtk+-3.0", "x_1", &"a".repeat(64)] {
            assert!(is_library_name(good), "{good}");
        }
        for bad in ["", "a b", "-lfoo;rm", "a/b", "é", &"a".repeat(65), "a\0"] {
            assert!(!is_library_name(bad), "{bad}");
        }
    }
}
