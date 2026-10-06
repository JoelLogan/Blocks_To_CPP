//! The desktop app's platform hardening (`docs/spec/08-security.md`
//! §8.6–§8.8):
//!
//! * the Windows application manifest keeps Common Controls v6 and declares
//!   `longPathAware`: in `windows-app-manifest.xml`, and on Windows with
//!   MSVC in this very test executable, where the linker embeds it like in
//!   every executable of the crate (`tools/check-windows-manifest.ps1` checks
//!   the app's own executable the same way in CI);
//! * the build script keeps embedding it through the linker;
//! * restricting the DLL search order is the first thing the app does;
//! * the main window adds the report-only Trusted Types policy (the policy
//!   itself is unit-tested in `src/window.rs`);
//! * a project saved under a path longer than 260 characters can be saved
//!   again, reopened and built (on the Windows runners, with
//!   `LongPathsEnabled`, this is what `longPathAware` is for).
//!
//! `B2C_CHECK_EXE=<path>` checks the manifest of another Windows
//! executable, such as a release build, on any platform.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use b2c_app::{Dialogs, TrustChoice, TrustPrompt};
use common::{Harness, gxx};
use serde_json::{Value, json};

fn manifest_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn read(relative: &str) -> String {
    fs::read_to_string(manifest_dir().join(relative)).unwrap()
}

/// The XML namespaces of the two entries.
const ASM_V1: &str = "urn:schemas-microsoft-com:asm.v1";
const ASM_V3: &str = "urn:schemas-microsoft-com:asm.v3";
const WINDOWS_SETTINGS_2016: &str = "http://schemas.microsoft.com/SMI/2016/WindowsSettings";

// ---------------------------------------------------------------------------
// A small namespace-aware reader for application manifests
// ---------------------------------------------------------------------------

/// One element of a manifest, with its namespace resolved.
#[derive(Debug, Clone)]
struct Element {
    namespace: String,
    name: String,
    attributes: Vec<(String, String)>,
    /// The text directly inside the element.
    text: String,
    /// The `(namespace, name)` of each ancestor, outermost first.
    path: Vec<(String, String)>,
}

impl Element {
    fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

/// The elements of `xml` in document order. Enough XML for application
/// manifests as tools write them (an XML declaration, processing
/// instructions, comments, elements, attributes in either quote, text,
/// default and prefixed namespaces); anything else (a DTD, CDATA,
/// unbalanced tags) is an error, so a check never passes by misreading.
///
/// The XML declaration (`<?xml …?>`) must be the very first thing, after an
/// optional byte-order mark: one after anything else, even whitespace, is
/// not well-formed, and Windows, .NET and `tools/check-windows-manifest.ps1`
/// reject it.
fn elements(xml: &str) -> Result<Vec<Element>, String> {
    let mut found: Vec<Element> = Vec::new();
    // Open elements: index into `found`, and the namespace bindings in scope.
    let mut open: Vec<(usize, Vec<(String, String)>)> = Vec::new();
    let document = xml.strip_prefix('\u{feff}').unwrap_or(xml);
    let mut rest = document;
    while let Some(start) = rest.find('<') {
        if let Some((index, _)) = open.last() {
            found[*index].text.push_str(&rest[..start]);
        }
        rest = &rest[start..];
        if let Some(after) = rest.strip_prefix("<?") {
            let target = after
                .split(|c: char| c.is_whitespace() || c == '?')
                .next()
                .unwrap_or_default();
            if target.eq_ignore_ascii_case("xml") && rest.len() != document.len() {
                return Err("the XML declaration is not the very first thing in the manifest".to_owned());
            }
            rest = &after[after.find("?>").ok_or("unterminated declaration")? + 2..];
        } else if let Some(after) = rest.strip_prefix("<!--") {
            rest = &after[after.find("-->").ok_or("unterminated comment")? + 3..];
        } else if rest.starts_with("<!") {
            return Err("DTDs and CDATA are not expected in a manifest".to_owned());
        } else if let Some(after) = rest.strip_prefix("</") {
            let end = after.find('>').ok_or("unterminated end tag")?;
            let (index, _) = open.pop().ok_or("an end tag without a start tag")?;
            let qualified = after[..end].trim();
            let local = qualified.rsplit(':').next().unwrap_or(qualified);
            if local != found[index].name {
                return Err(format!("</{qualified}> closes <{}>", found[index].name));
            }
            rest = &after[end + 1..];
        } else {
            let (tag, self_closing, after) = start_tag(&rest[1..])?;
            let mut scope = open.last().map(|(_, scope)| scope.clone()).unwrap_or_default();
            let mut attributes = Vec::new();
            for (key, value) in tag.attributes {
                if key == "xmlns" {
                    scope.push((String::new(), value));
                } else if let Some(prefix) = key.strip_prefix("xmlns:") {
                    scope.push((prefix.to_owned(), value));
                } else {
                    attributes.push((key, value));
                }
            }
            let (prefix, name) = tag.name.split_once(':').unwrap_or(("", &tag.name));
            let namespace = scope
                .iter()
                .rev()
                .find(|(bound, _)| bound == prefix)
                .map(|(_, uri)| uri.clone())
                .ok_or_else(|| format!("no namespace for <{}>", tag.name))?;
            let path = open
                .iter()
                .map(|(index, _)| (found[*index].namespace.clone(), found[*index].name.clone()))
                .collect();
            found.push(Element {
                namespace,
                name: name.to_owned(),
                attributes,
                text: String::new(),
                path,
            });
            if !self_closing {
                open.push((found.len() - 1, scope));
            }
            rest = after;
        }
    }
    if open.is_empty() {
        Ok(found)
    } else {
        Err("unclosed elements".to_owned())
    }
}

/// A start tag's name and attributes.
struct StartTag {
    name: String,
    attributes: Vec<(String, String)>,
}

/// Reads a start tag from `text` (just after its `<`): the tag, whether it
/// closes itself, and what follows it.
fn start_tag(text: &str) -> Result<(StartTag, bool, &str), String> {
    let name_end = text
        .find(|c: char| c.is_whitespace() || c == '/' || c == '>')
        .ok_or("unterminated start tag")?;
    let name = text[..name_end].to_owned();
    let mut rest = &text[name_end..];
    let mut attributes = Vec::new();
    loop {
        rest = rest.trim_start();
        if let Some(after) = rest.strip_prefix("/>") {
            return Ok((StartTag { name, attributes }, true, after));
        }
        if let Some(after) = rest.strip_prefix('>') {
            return Ok((StartTag { name, attributes }, false, after));
        }
        let equals = rest
            .find('=')
            .ok_or_else(|| format!("bad attribute in <{name}>"))?;
        let key = rest[..equals].trim().to_owned();
        let value = rest[equals + 1..].trim_start();
        let quote = value
            .chars()
            .next()
            .filter(|c| *c == '"' || *c == '\'')
            .ok_or_else(|| format!("unquoted attribute {key} in <{name}>"))?;
        let value = &value[1..];
        let end = value
            .find(quote)
            .ok_or_else(|| format!("unterminated attribute {key} in <{name}>"))?;
        attributes.push((key, value[..end].to_owned()));
        rest = &value[end + 1..];
    }
}

/// Checks that a manifest has both entries the app needs: the
/// Common Controls v6 dependency and `longPathAware` set to `true` under
/// `application/windowsSettings`. The same checks as
/// `tools/check-windows-manifest.ps1`.
fn check_manifest(xml: &str) -> Result<(), String> {
    let elements = elements(xml)?;
    let in_ns = |element: &Element, namespace: &str, name: &str| {
        element.namespace == namespace && element.name == name
    };
    let root = elements.first().ok_or("an empty manifest")?;
    if !in_ns(root, ASM_V1, "assembly") {
        return Err(format!("the root is <{}> in {}", root.name, root.namespace));
    }
    let common_controls = elements.iter().any(|element| {
        in_ns(element, ASM_V1, "assemblyIdentity")
            && element.path.iter().map(|(_, name)| name.as_str()).eq([
                "assembly",
                "dependency",
                "dependentAssembly",
            ])
            && element.attribute("type") == Some("win32")
            && element.attribute("name") == Some("Microsoft.Windows.Common-Controls")
            && element.attribute("version") == Some("6.0.0.0")
            && element.attribute("publicKeyToken") == Some("6595b64144ccf1df")
            && element.attribute("processorArchitecture") == Some("*")
    });
    if !common_controls {
        return Err("no Microsoft.Windows.Common-Controls 6.0.0.0 dependency".to_owned());
    }
    let long_paths = elements.iter().any(|element| {
        in_ns(element, WINDOWS_SETTINGS_2016, "longPathAware")
            && element.text.trim().eq_ignore_ascii_case("true")
            && element.path.len() >= 2
            && element.path[element.path.len() - 2..]
                .iter()
                .map(|(_, name)| name.as_str())
                .eq(["application", "windowsSettings"])
            && element.path[element.path.len() - 2].0 == ASM_V3
    });
    if !long_paths {
        return Err("no longPathAware = true under application/windowsSettings".to_owned());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The manifest embedded in a Windows executable
// ---------------------------------------------------------------------------

/// `RT_MANIFEST`, and the ID of an executable's own manifest
/// (`CREATEPROCESS_MANIFEST_RESOURCE_ID`).
const RT_MANIFEST: u32 = 24;
const PROCESS_MANIFEST_ID: u32 = 1;

fn u16_at(image: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        image.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn u32_at(image: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        image.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

/// The process manifest embedded in the PE image `image` (the `RT_MANIFEST`
/// resource with ID 1, in its first language), or `None` when the image has
/// none or is not a PE image.
fn embedded_manifest(image: &[u8]) -> Option<Vec<u8>> {
    let usize_at = |offset: usize| u32_at(image, offset).and_then(|value| usize::try_from(value).ok());
    if image.get(..2)? != b"MZ" {
        return None;
    }
    let pe = usize_at(0x3c)?;
    if image.get(pe..pe + 4)? != b"PE\0\0" {
        return None;
    }
    let coff = pe + 4;
    let sections = usize::from(u16_at(image, coff + 2)?);
    let optional = coff + 20;
    let optional_size = usize::from(u16_at(image, coff + 16)?);
    // PE32 or PE32+: where the data directories start.
    let (count_at, directories) = match u16_at(image, optional)? {
        0x10b => (optional + 92, optional + 96),
        0x20b => (optional + 108, optional + 112),
        _ => return None,
    };
    // The resource table is data directory 2.
    if usize_at(count_at)? <= 2 {
        return None;
    }
    let resources_rva = usize_at(directories + 2 * 8)?;
    let section_table = optional + optional_size;
    let to_offset = |rva: usize| {
        (0..sections).find_map(|index| {
            let header = section_table + index * 40;
            let virtual_size = usize_at(header + 8)?;
            let address = usize_at(header + 12)?;
            let raw_size = usize_at(header + 16)?;
            let raw = usize_at(header + 20)?;
            (rva >= address && rva < address + virtual_size.max(raw_size)).then(|| raw + (rva - address))
        })
    };
    let base = to_offset(resources_rva)?;
    // One level of the resource tree: the entry with `id` (or the first entry
    // when `id` is `None`), as (is a subdirectory, offset of its target).
    let entry = |directory: usize, id: Option<u32>| -> Option<(bool, usize)> {
        let named = usize::from(u16_at(image, directory + 12)?);
        let ids = usize::from(u16_at(image, directory + 14)?);
        (0..named + ids).find_map(|index| {
            let at = directory + 16 + index * 8;
            let name = u32_at(image, at)?;
            if id.is_some_and(|id| name != id) {
                return None;
            }
            let target = u32_at(image, at + 4)?;
            let offset = usize::try_from(target & 0x7fff_ffff).ok()?;
            Some((target & 0x8000_0000 != 0, base + offset))
        })
    };
    let (true, names) = entry(base, Some(RT_MANIFEST))? else {
        return None;
    };
    let (true, languages) = entry(names, Some(PROCESS_MANIFEST_ID))? else {
        return None;
    };
    let (false, data) = entry(languages, None)? else {
        return None;
    };
    let start = to_offset(usize_at(data)?)?;
    let size = usize_at(data + 4)?;
    Some(image.get(start..start + size)?.to_vec())
}

/// Checks the manifest embedded in the executable at `path`.
fn check_executable(path: &Path) {
    let image = fs::read(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let manifest = embedded_manifest(&image)
        .unwrap_or_else(|| panic!("{} has no embedded process manifest", path.display()));
    let text = String::from_utf8(manifest).unwrap();
    if let Err(problem) = check_manifest(&text) {
        panic!("{}: {problem}\n{text}", path.display());
    }
}

#[test]
fn the_manifest_source_has_both_entries() {
    let xml = read("windows-app-manifest.xml");
    check_manifest(&xml).unwrap();
    // ASCII only: the linker, windres and mt merge it, and none of them
    // needs to guess an encoding.
    assert!(xml.is_ascii());
}

#[test]
fn the_manifest_check_finds_what_is_missing() {
    let good = read("windows-app-manifest.xml");
    let without_common_controls = good.replace("Microsoft.Windows.Common-Controls", "Other");
    assert!(
        check_manifest(&without_common_controls)
            .unwrap_err()
            .contains("Common-Controls")
    );
    let old_version = good.replace("version=\"6.0.0.0\"", "version=\"5.82.0.0\"");
    assert!(check_manifest(&old_version).is_err());
    let off = good.replace(">true</longPathAware>", ">false</longPathAware>");
    assert!(check_manifest(&off).unwrap_err().contains("longPathAware"));
    let wrong_namespace = good.replace(
        WINDOWS_SETTINGS_2016,
        "http://schemas.microsoft.com/SMI/2005/WindowsSettings",
    );
    assert!(check_manifest(&wrong_namespace).is_err());
    let outside_settings = good
        .replace("<windowsSettings>", "<windowsSettings></windowsSettings><other>")
        .replace(
            "</windowsSettings>\n  </application>",
            "</other>\n  </application>",
        );
    assert!(check_manifest(&outside_settings).is_err(), "{outside_settings}");
    assert!(check_manifest("<assembly xmlns=\"urn:schemas-microsoft-com:asm.v1\"><dependency>").is_err());
    assert!(check_manifest("<!DOCTYPE x><assembly/>").is_err());
}

/// An XML declaration counts only at the very start, after an optional
/// byte-order mark.
#[test]
fn an_xml_declaration_must_come_first() {
    let good = read("windows-app-manifest.xml");
    let declaration = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#;
    check_manifest(&format!("{declaration}\n{good}")).unwrap();
    check_manifest(&format!("\u{feff}{declaration}\n{good}")).unwrap();
    for bad in [
        // What tauri-winres made of the manifest while it had a declaration.
        format!(" {declaration} {good}"),
        format!("\n{declaration}\n{good}"),
        format!("\u{feff}\u{feff}{declaration}\n{good}"),
        format!("<!-- first -->{declaration}\n{good}"),
        format!("{declaration}{declaration}\n{good}"),
        format!("{good}{declaration}"),
        format!(" <?XML version=\"1.0\"?>{good}"),
    ] {
        let problem = check_manifest(&bad).unwrap_err();
        assert!(problem.contains("XML declaration"), "{problem}: {bad}");
    }
    // Other processing instructions may come anywhere.
    check_manifest(&format!("{good}<?xml-stylesheet href='a.xsl'?>")).unwrap();
}

/// With MinGW, tauri-build hands the manifest to tauri-winres, which writes
/// every line of it into the resource file trimmed and between spaces, so
/// the embedded manifest starts with a space (`build.rs`). It must still be
/// well-formed there, which is why the file has no XML declaration.
#[test]
fn the_manifest_survives_the_mingw_resource_file() {
    // Each line becomes the string literal `" <line> "`, and the resource
    // compiler joins consecutive literals.
    let mut embedded = String::new();
    for line in read("windows-app-manifest.xml").lines() {
        embedded.push(' ');
        embedded.push_str(line.trim());
        embedded.push(' ');
    }
    assert!(embedded.starts_with(' '));
    check_manifest(&embedded).unwrap();
}

/// Tools that merge manifests (`mt.exe`, the MSVC linker) may move a
/// namespace onto a prefix; that must still count.
#[test]
fn prefixed_namespaces_are_resolved() {
    let merged = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns='{ASM_V1}' manifestVersion='1.0'>
  <trustInfo xmlns="{ASM_V3}"><security><requestedPrivileges>
    <requestedExecutionLevel level="asInvoker" uiAccess="false"></requestedExecutionLevel>
  </requestedPrivileges></security></trustInfo>
  <dependency><dependentAssembly>
    <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0"
      processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
  </dependentAssembly></dependency>
  <ms_asmv3:application xmlns:ms_asmv3="{ASM_V3}">
    <ms_asmv3:windowsSettings>
      <ms_windowsSettings:longPathAware xmlns:ms_windowsSettings="{WINDOWS_SETTINGS_2016}"> TRUE </ms_windowsSettings:longPathAware>
    </ms_asmv3:windowsSettings>
  </ms_asmv3:application>
</assembly>"#
    );
    check_manifest(&merged).unwrap();
}

#[test]
fn a_pe_image_without_resources_has_no_manifest() {
    assert_eq!(embedded_manifest(b""), None);
    assert_eq!(embedded_manifest(b"MZ"), None);
    assert_eq!(embedded_manifest(&[0_u8; 4096]), None);
    let mut truncated = vec![0_u8; 0x80];
    truncated[..2].copy_from_slice(b"MZ");
    truncated[0x3c] = 0x40;
    truncated[0x40..0x44].copy_from_slice(b"PE\0\0");
    assert_eq!(embedded_manifest(&truncated), None);
}

/// With MSVC the linker embeds the manifest in every executable of the
/// crate, this test binary included; read it back from the file.
#[cfg(all(windows, target_env = "msvc"))]
#[test]
fn this_executable_embeds_the_manifest() {
    check_executable(&std::env::current_exe().unwrap());
}

/// `B2C_CHECK_EXE=<path to an .exe>`: checks that executable's manifest.
#[test]
fn a_given_executable_embeds_the_manifest() {
    if let Some(path) = std::env::var_os("B2C_CHECK_EXE") {
        check_executable(Path::new(&path));
    }
}

// ---------------------------------------------------------------------------
// Build script, start-up order and the Trusted Types wiring
// ---------------------------------------------------------------------------

/// The manifest reaches every MSVC executable through the linker, never
/// through tauri-build's resource file, which only the app binary gets
/// (test binaries without Common Controls v6 do not start).
#[test]
fn the_build_script_embeds_the_manifest_through_the_linker() {
    let build = read("build.rs");
    for needle in [
        "const MANIFEST: &str = \"windows-app-manifest.xml\";",
        "cargo:rustc-link-arg=/MANIFEST:EMBED",
        "cargo:rustc-link-arg=/MANIFESTINPUT:{manifest}",
        "WindowsAttributes::new_without_app_manifest()",
    ] {
        assert!(build.contains(needle), "build.rs lost {needle:?}");
    }
}

/// The first statement of the function whose header is `header` in `source`
/// (comment lines skipped).
fn first_statement<'a>(source: &'a str, header: &str) -> &'a str {
    let (_, body) = source
        .split_once(header)
        .unwrap_or_else(|| panic!("{header:?} not found"));
    body.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("//"))
        .unwrap()
}

/// 08 §8.7: nothing runs before the DLL hardening that could load a DLL by
/// name: `main` only calls `run`, and `run` hardens first.
#[test]
fn dll_hardening_is_the_first_thing_the_app_does() {
    let main = read("src/main.rs");
    assert_eq!(
        first_statement(&main, "fn main() -> ExitCode {"),
        "blocks2cpp_desktop::run()"
    );
    let lib = read("src/lib.rs");
    assert_eq!(
        first_statement(&lib, "pub fn run() -> ExitCode {"),
        "let dll_search = b2c_build::os::harden_dll_search();"
    );
}

#[test]
fn the_main_window_adds_the_trusted_types_policy() {
    let window = read("src/window.rs");
    let (_, open) = window.split_once("pub(crate) fn open_main_window").unwrap();
    let (open, _) = open.split_once(".build()?").unwrap();
    assert!(open.contains(".on_web_resource_request("), "{open}");
    assert!(
        open.contains("add_trusted_types_report_only(request.uri(), response)"),
        "{open}"
    );
}

// ---------------------------------------------------------------------------
// Long paths
// ---------------------------------------------------------------------------

/// Dialogs whose Save As picks a fixed file; everything else is cancelled.
struct SaveAt(PathBuf);

impl Dialogs for SaveAt {
    fn open_project(&self) -> Option<PathBuf> {
        None
    }
    fn save_project_as(&self, _suggested_file_name: &str) -> Option<PathBuf> {
        Some(self.0.clone())
    }
    fn pick_compiler(&self) -> Option<PathBuf> {
        None
    }
    fn confirm_trust(&self, _prompt: &TrustPrompt) -> TrustChoice {
        TrustChoice::StayRestricted
    }
}

/// A folder under `root` whose path is longer than `MAX_PATH` (260
/// characters), each part well within the 255-character limit.
fn long_folder(root: &Path) -> PathBuf {
    let mut folder = root.to_path_buf();
    let mut level = 0;
    while folder.as_os_str().len() <= 300 {
        folder.push(format!(
            "{level}-a-folder-with-a-rather-long-name-for-blocks2cpp-projects"
        ));
        level += 1;
    }
    fs::create_dir_all(&folder).unwrap();
    folder
}

/// 08 §8.6, 02 §2.8: a project under a path longer than 260 characters can
/// be saved, saved again (atomic replace and `.bak`), reopened and built.
/// On Windows this runs with the long-path manifest (and the runner's
/// `LongPathsEnabled`).
#[test]
fn a_project_under_a_long_path_saves_reopens_and_builds() {
    let compiler = gxx();
    let projects = tempfile::tempdir().unwrap();
    let file = long_folder(projects.path()).join("long-path-game.b2c");
    assert!(file.as_os_str().len() > 260);
    let toolchains = compiler
        .iter()
        .map(|compiler| compiler.parent().unwrap().to_path_buf())
        .collect();
    let app = Harness::new(toolchains, Arc::new(SaveAt(file.clone())), true);

    let created = app
        .invoke("project_new", json!({ "request": { "template": "helloWorld" } }))
        .unwrap();
    let request = json!({ "request": { "handle": created["handle"], "document": created["document"] } });
    let saved = app.invoke("project_save_as_dialog", request.clone()).unwrap();
    assert_eq!(saved["status"], "ok", "{saved}");
    assert_eq!(saved["fileName"], "long-path-game.b2c");
    let on_disk = fs::read_to_string(&file).unwrap();
    assert_eq!(Value::from(on_disk.as_str()), created["document"]);

    let saved_again = app.invoke("project_save", request).unwrap();
    assert_eq!(saved_again["hash"], saved["hash"], "{saved_again}");
    assert!(file.with_file_name("long-path-game.b2c.bak").is_file());

    app.invoke(
        "project_close",
        json!({ "request": { "handle": created["handle"] } }),
    )
    .unwrap();
    let recent = app.invoke("recent_list", json!({})).unwrap();
    let entry = &recent["entries"][0];
    assert!(entry["projectName"].is_string(), "{recent}");
    let opened = app
        .invoke(
            "project_open_recent",
            json!({ "request": { "recentId": entry["recentId"] } }),
        )
        .unwrap();
    assert_eq!(opened["document"], created["document"]);
    assert_eq!(opened["fileName"], "long-path-game.b2c");

    if compiler.is_none() {
        return;
    }
    app.invoke(
        "build_start",
        json!({
            "request": { "handle": opened["handle"], "document": opened["document"], "config": "debug" },
            "onEvent": "__CHANNEL__:3",
        }),
    )
    .unwrap();
    let events = app.wait_for(3, "finished");
    assert_eq!(events.last().unwrap()["outcome"], "built", "{events:?}");
}
