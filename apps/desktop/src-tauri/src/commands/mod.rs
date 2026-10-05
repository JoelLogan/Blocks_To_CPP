//! The IPC command adapters (`docs/spec/02-architecture.md` §2.5.2), one per
//! command of [`b2c_ipc::COMMANDS`], grouped by area. They hold no logic:
//!
//! 1. take the `request` argument as JSON (`None` when it is missing) and the
//!    channel arguments (`onEvent`, `onOutput`);
//! 2. on Tauri's blocking thread pool, decode the request with
//!    [`b2c_ipc::decode()`] (schema, serde, validation: every failure is a
//!    typed `invalidRequest` or `payloadTooLarge`) and call the one
//!    [`Backend`] method of the command;
//! 3. return its `Result`, whose error serialises as `{ "code": … }`.
//!
//! A panic in the backend is caught here and returned as `internal`; it never
//! crosses the IPC boundary (`docs/spec/09-quality-and-delivery.md` §9.1). The
//! panic hook logs where it happened (see [`crate::logging`]).
//!
//! The isolation hook already checked each message's keys and shapes; the
//! backend checks everything again, because a compromised webview could call
//! `__TAURI_INTERNALS__` directly (`docs/spec/08-security.md` §8.8).

pub(crate) mod app;
pub(crate) mod build;
pub(crate) mod help;
pub(crate) mod project;
pub(crate) mod recent;
pub(crate) mod recovery;
pub(crate) mod run;
pub(crate) mod settings;
pub(crate) mod toolchain;
pub(crate) mod trust;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use b2c_app::Backend;
use b2c_ipc::{InvalidReason, IpcError, IpcRequest};
use serde_json::Value;
use tauri::State;

/// The backend, as every command receives it.
pub(crate) type BackendState<'a> = State<'a, Arc<Backend>>;

/// A shared reference to the backend that can move to another thread.
pub(crate) fn shared(state: &BackendState<'_>) -> Arc<Backend> {
    Arc::clone(state.inner())
}

/// Decodes a command's `request` argument: `None` (the key is missing or
/// `null`) is a `missingField` error, anything else goes through
/// [`b2c_ipc::decode()`].
///
/// # Errors
/// [`IpcError::InvalidRequest`] or [`IpcError::PayloadTooLarge`]; nothing has
/// changed when it fails.
pub(crate) fn decode<T: IpcRequest>(request: Option<Value>) -> Result<T, IpcError> {
    let value = request.ok_or_else(|| IpcError::invalid(InvalidReason::MissingField, None))?;
    b2c_ipc::decode(value)
}

/// Runs `work`, turning a panic into [`IpcError::Internal`]. The panic hook has
/// already logged the panic's location; its message is never logged or sent.
pub(crate) fn catch<T>(
    command: &'static str,
    work: impl FnOnce() -> Result<T, IpcError>,
) -> Result<T, IpcError> {
    catch_unwind(AssertUnwindSafe(work)).unwrap_or_else(|_| {
        tracing::error!(command, "a command panicked; it returned `internal`");
        Err(IpcError::Internal)
    })
}

/// Runs one command's work on Tauri's blocking thread pool (commands may
/// wait for a dialog, the disk or a compiler probe; 02 §2.6), catching panics
/// ([`catch`]).
///
/// # Errors
/// The command's own error, or [`IpcError::Internal`] when it panicked or its
/// thread could not run.
pub(crate) async fn blocking<T, F>(command: &'static str, work: F) -> Result<T, IpcError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, IpcError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || catch(command, work))
        .await
        .unwrap_or_else(|_| {
            // Only when the runtime is shutting down: the closure itself
            // cannot panic past `catch`.
            tracing::error!(command, "a command's thread did not finish");
            Err(IpcError::Internal)
        })
}

#[cfg(test)]
mod tests {
    use b2c_ipc::dto::{ProjectSaveRequest, RunInputRequest};
    use serde_json::json;

    use super::*;

    const HANDLE: &str = "ph_0123456789abcdef0123456789abcdef";

    #[test]
    fn a_missing_request_is_a_typed_error() {
        let error = decode::<ProjectSaveRequest>(None).unwrap_err();
        assert_eq!(error, IpcError::invalid(InvalidReason::MissingField, None));
        assert_eq!(serde_json::to_value(&error).unwrap()["code"], "invalidRequest");
    }

    #[test]
    fn requests_are_decoded_strictly() {
        let unknown = json!({ "handle": HANDLE, "document": "{}", "extra": 1 });
        assert!(matches!(
            decode::<ProjectSaveRequest>(Some(unknown)),
            Err(IpcError::InvalidRequest {
                reason: InvalidReason::UnknownField,
                ..
            })
        ));
        let forged = json!({ "handle": "ph_../../etc", "document": "{}" });
        assert!(matches!(
            decode::<ProjectSaveRequest>(Some(forged)),
            Err(IpcError::InvalidRequest {
                reason: InvalidReason::BadId,
                ..
            })
        ));
        assert!(matches!(
            decode::<ProjectSaveRequest>(Some(Value::Null)),
            Err(IpcError::InvalidRequest {
                reason: InvalidReason::Malformed,
                ..
            })
        ));
        let input = json!({ "runId": "rn_0123456789abcdef0123456789abcdef", "data": "aGk=" });
        assert_eq!(
            decode::<RunInputRequest>(Some(input)).unwrap().bytes().unwrap(),
            b"hi"
        );
    }

    #[test]
    fn oversized_input_is_refused_before_it_is_used() {
        // 65,536 bytes of program input are allowed; one more is not.
        let data = b2c_ipc::encode_base64(&vec![0_u8; 65_537]);
        let input = json!({ "runId": "rn_0123456789abcdef0123456789abcdef", "data": data });
        assert!(matches!(
            decode::<RunInputRequest>(Some(input)),
            Err(IpcError::PayloadTooLarge { .. })
        ));
    }

    #[test]
    fn panics_become_internal_errors() {
        let result: Result<(), IpcError> = catch("test", || panic!("secret project text"));
        assert_eq!(result, Err(IpcError::Internal));
        assert_eq!(catch("test", || Ok(7)), Ok(7));
        assert_eq!(
            catch::<()>("test", || Err(IpcError::NotFound)),
            Err(IpcError::NotFound)
        );
    }

    #[test]
    fn blocking_work_runs_off_the_calling_thread_and_catches_panics() {
        let caller = std::thread::current().id();
        let ran_on =
            tauri::async_runtime::block_on(blocking("test", move || Ok(std::thread::current().id())));
        assert_ne!(ran_on.unwrap(), caller);
        let panicked: Result<(), IpcError> =
            tauri::async_runtime::block_on(blocking("test", || panic!("secret project text")));
        assert_eq!(panicked, Err(IpcError::Internal));
    }
}
