//! Format migrations (spec §5.7): a chain of pure functions, each upgrading
//! a parsed file from format version `n` to `n + 1`.
//!
//! Migrations work on the parsed JSON tree, not on [`crate::Document`]: an
//! older file does not fit the current types (keys may have been renamed or
//! restructured), so it is upgraded first and then decoded and validated
//! exactly like a current file. Version 1 is the first format, so the chain
//! is empty for now; the tests below exercise the machinery with a
//! synthetic chain.

use crate::json::Json;

/// One step of the chain.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Migration {
    /// The version this step reads; it produces `from + 1`.
    pub(crate) from: u32,
    /// The upgrade. It may restructure anything except `formatVersion`,
    /// which the chain sets. An error is a plain-English reason.
    pub(crate) apply: fn(Json) -> Result<Json, String>,
}

/// The migrations shipped with this version, ordered by `from`.
pub(crate) const MIGRATIONS: &[Migration] = &[];

/// Why a file could not be upgraded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MigrationError {
    /// No step reads this version.
    NoPath {
        /// The version that cannot be upgraded.
        from: u32,
    },
    /// A step failed.
    Failed {
        /// The version the failing step reads.
        from: u32,
        /// Why it failed.
        reason: String,
    },
}

/// Upgrades `root` from version `from` to version `to` with `chain`.
///
/// # Errors
/// Fails when a step is missing or reports an error. `from > to` is a
/// missing step too (downgrades do not exist).
pub(crate) fn run_chain(
    mut root: Json,
    from: u32,
    to: u32,
    chain: &[Migration],
) -> Result<Json, MigrationError> {
    if from > to {
        return Err(MigrationError::NoPath { from });
    }
    let mut version = from;
    while version < to {
        let step = chain
            .iter()
            .find(|step| step.from == version)
            .ok_or(MigrationError::NoPath { from: version })?;
        root = (step.apply)(root).map_err(|reason| MigrationError::Failed {
            from: version,
            reason,
        })?;
        version += 1;
        root.set("formatVersion", Json::Number(version.into()));
    }
    Ok(root)
}

/// Upgrades a file of version `from` to [`crate::CURRENT_FORMAT_VERSION`].
///
/// # Errors
/// See [`run_chain`].
pub(crate) fn upgrade(root: Json, from: u32) -> Result<Json, MigrationError> {
    run_chain(root, from, crate::CURRENT_FORMAT_VERSION, MIGRATIONS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::parse;

    /// A synthetic v1 → v2 step: `project.title` was renamed to `project.name`.
    fn rename_title(mut root: Json) -> Result<Json, String> {
        let Json::Object(entries) = &mut root else {
            return Err("not an object".into());
        };
        let project = entries
            .iter_mut()
            .find(|(k, _)| &**k == "project")
            .map(|(_, v)| v)
            .ok_or("no project")?;
        let title = project.remove("title").ok_or("no title")?;
        project.set("name", title);
        Ok(root)
    }

    /// A synthetic v2 → v3 step that adds a key.
    #[allow(clippy::unnecessary_wraps, reason = "the signature every migration has")]
    fn add_flag(mut root: Json) -> Result<Json, String> {
        root.set("flag", Json::Bool(true));
        Ok(root)
    }

    fn failing(_: Json) -> Result<Json, String> {
        Err("this step always fails".into())
    }

    const CHAIN: &[Migration] = &[
        Migration {
            from: 2,
            apply: add_flag,
        },
        Migration {
            from: 1,
            apply: rename_title,
        },
    ];

    fn json(text: &str) -> Json {
        parse(text).unwrap().value
    }

    #[test]
    fn chain_upgrades_step_by_step() {
        let old = json(r#"{"formatVersion":1,"project":{"title":"Game"}}"#);
        let upgraded = run_chain(old, 1, 3, CHAIN).unwrap();
        assert_eq!(
            upgraded,
            json(r#"{"formatVersion":3,"project":{"name":"Game"},"flag":true}"#)
        );
    }

    #[test]
    fn partial_chains_and_no_op() {
        let old = json(r#"{"formatVersion":2}"#);
        assert_eq!(
            run_chain(old.clone(), 2, 3, CHAIN).unwrap(),
            json(r#"{"formatVersion":3,"flag":true}"#)
        );
        assert_eq!(run_chain(old.clone(), 2, 2, CHAIN).unwrap(), old);
    }

    #[test]
    fn missing_steps_and_failures() {
        let old = json(r#"{"formatVersion":0}"#);
        assert_eq!(
            run_chain(old.clone(), 0, 3, CHAIN),
            Err(MigrationError::NoPath { from: 0 })
        );
        assert_eq!(
            run_chain(old.clone(), 4, 3, CHAIN),
            Err(MigrationError::NoPath { from: 4 })
        );
        assert_eq!(
            run_chain(json(r#"{"project":{}}"#), 1, 2, CHAIN),
            Err(MigrationError::Failed {
                from: 1,
                reason: "no title".into()
            })
        );
        let failing_chain = [Migration {
            from: 1,
            apply: failing,
        }];
        assert!(matches!(
            run_chain(old, 1, 2, &failing_chain),
            Err(MigrationError::Failed { from: 1, .. })
        ));
    }

    #[test]
    fn shipped_chain_is_consistent() {
        // Every shipped step reads an older version than the current one,
        // and there is exactly one step per version.
        for (index, step) in MIGRATIONS.iter().enumerate() {
            assert!(step.from < crate::CURRENT_FORMAT_VERSION);
            assert!(MIGRATIONS.iter().skip(index + 1).all(|s| s.from != step.from));
        }
        let current = json(r#"{"formatVersion":1}"#);
        assert_eq!(
            upgrade(current.clone(), crate::CURRENT_FORMAT_VERSION),
            Ok(current.clone())
        );
        assert_eq!(upgrade(current, 0), Err(MigrationError::NoPath { from: 0 }));
    }
}
