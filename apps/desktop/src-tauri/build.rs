//! Build script: embeds the Tauri configuration, the capability files and (on
//! Windows) the icon and manifest. It also generates permissions for our own
//! commands, so the webview can call only the commands a capability grants.

use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    // Listing the commands here makes each one need an explicit `allow-…`
    // permission in `capabilities/` (docs/spec/08-security.md §8.8).
    let commands = tauri_build::AppManifest::new().commands(&["app_version"]);
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(commands))?;
    Ok(())
}
