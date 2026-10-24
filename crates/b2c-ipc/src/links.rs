//! The fixed help links that `open_help_link` may open
//! (`docs/spec/08-security.md` §8.8 "No remote content").
//!
//! The webview names a link only by its [`LinkId`]; it can never pass a URL. The
//! backend opens [`LinkId::url`] in the OS browser.

use crate::macros::string_enum;

string_enum! {
    /// A documentation page the app may open in the OS browser.
    pub enum LinkId {
        /// MSYS2's installation page (the recommended g++ on Windows).
        Msys2Install = "msys2Install",
        /// `WinLibs`, a standalone `MinGW-w64` g++ build for Windows.
        Winlibs = "winlibs",
        /// The diagnostics reference: every `B2C-…` code with an explanation.
        DiagnosticsReference = "diagnosticsReference",
    }
}

impl LinkId {
    /// The fixed `https` URL of this link.
    ///
    /// The diagnostics reference lives in the repository
    /// (`docs/reference/diagnostics/`). The specification website publishes only
    /// the specification and the decision records (`site/build.mjs`) and links to
    /// every other repository file on GitHub, so this is the same URL the website
    /// uses for the reference.
    pub const fn url(self) -> &'static str {
        match self {
            Self::Msys2Install => "https://www.msys2.org/",
            Self::Winlibs => "https://winlibs.com/",
            Self::DiagnosticsReference => {
                "https://github.com/JoelLogan/Blocks_To_CPP/blob/HEAD/docs/reference/diagnostics/README.md"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_link_is_a_fixed_https_url() {
        for link in LinkId::ALL {
            let url = link.url();
            let rest = url.strip_prefix("https://").unwrap();
            let host = rest.split('/').next().unwrap();
            assert!(!host.is_empty() && !host.contains(['@', ':']), "{url}");
            assert!(
                url.bytes().all(|b| b.is_ascii_graphic()),
                "{url} must be plain ASCII"
            );
        }
        assert_eq!(LinkId::ALL.len(), 3);
    }

    #[test]
    fn ids_are_a_closed_set() {
        assert_eq!(
            serde_json::from_str::<LinkId>("\"winlibs\"").unwrap(),
            LinkId::Winlibs
        );
        assert!(serde_json::from_str::<LinkId>("\"https://example.com\"").is_err());
        assert!(serde_json::from_str::<LinkId>("\"Winlibs\"").is_err());
    }
}
