//! Standard headers (ADR-0017 §6; `docs/plans/m3-contracts/types.md` §5).
//!
//! A [`Header`] is a typed leaf like the leaves of [`crate::text`]: it comes
//! only from a closed table of the C++ standard library's headers, so no
//! project file, catalog entry or block can make the generator print an
//! `#include` of anything else. [`ItemHeaders`] records the catalog headers
//! of a top-level item by where its code is printed.

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::sast::CppStandard;

/// A standard header, made only by [`Header::standard`]. Orders by its text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Header(&'static str);

/// The standard library's headers from C++17 to C++26, in their `<…>` form,
/// sorted, each with the first standard that has it. The C library's
/// `<….h>` forms are left out, and so are the headers deprecated or removed
/// within these standards (`<ccomplex>`, `<ciso646>`, `<codecvt>`,
/// `<cstdalign>`, `<cstdbool>`, `<ctgmath>`, `<strstream>`).
const TABLE: [(&str, CppStandard); 115] = {
    use CppStandard::{Cpp17 as C17, Cpp20 as C20, Cpp23 as C23, Cpp26 as C26};
    [
        ("<algorithm>", C17),
        ("<any>", C17),
        ("<array>", C17),
        ("<atomic>", C17),
        ("<barrier>", C20),
        ("<bit>", C20),
        ("<bitset>", C17),
        ("<cassert>", C17),
        ("<cctype>", C17),
        ("<cerrno>", C17),
        ("<cfenv>", C17),
        ("<cfloat>", C17),
        ("<charconv>", C17),
        ("<chrono>", C17),
        ("<cinttypes>", C17),
        ("<climits>", C17),
        ("<clocale>", C17),
        ("<cmath>", C17),
        ("<compare>", C20),
        ("<complex>", C17),
        ("<concepts>", C20),
        ("<condition_variable>", C17),
        ("<contracts>", C26),
        ("<coroutine>", C20),
        ("<csetjmp>", C17),
        ("<csignal>", C17),
        ("<cstdarg>", C17),
        ("<cstddef>", C17),
        ("<cstdint>", C17),
        ("<cstdio>", C17),
        ("<cstdlib>", C17),
        ("<cstring>", C17),
        ("<ctime>", C17),
        ("<cuchar>", C17),
        ("<cwchar>", C17),
        ("<cwctype>", C17),
        ("<debugging>", C26),
        ("<deque>", C17),
        ("<exception>", C17),
        ("<execution>", C17),
        ("<expected>", C23),
        ("<filesystem>", C17),
        ("<flat_map>", C23),
        ("<flat_set>", C23),
        ("<format>", C20),
        ("<forward_list>", C17),
        ("<fstream>", C17),
        ("<functional>", C17),
        ("<future>", C17),
        ("<generator>", C23),
        ("<hazard_pointer>", C26),
        ("<hive>", C26),
        ("<initializer_list>", C17),
        ("<inplace_vector>", C26),
        ("<iomanip>", C17),
        ("<ios>", C17),
        ("<iosfwd>", C17),
        ("<iostream>", C17),
        ("<istream>", C17),
        ("<iterator>", C17),
        ("<latch>", C20),
        ("<limits>", C17),
        ("<linalg>", C26),
        ("<list>", C17),
        ("<locale>", C17),
        ("<map>", C17),
        ("<mdspan>", C23),
        ("<memory>", C17),
        ("<memory_resource>", C17),
        ("<meta>", C26),
        ("<mutex>", C17),
        ("<new>", C17),
        ("<numbers>", C20),
        ("<numeric>", C17),
        ("<optional>", C17),
        ("<ostream>", C17),
        ("<print>", C23),
        ("<queue>", C17),
        ("<random>", C17),
        ("<ranges>", C20),
        ("<ratio>", C17),
        ("<rcu>", C26),
        ("<regex>", C17),
        ("<scoped_allocator>", C17),
        ("<semaphore>", C20),
        ("<set>", C17),
        ("<shared_mutex>", C17),
        ("<simd>", C26),
        ("<source_location>", C20),
        ("<span>", C20),
        ("<spanstream>", C23),
        ("<sstream>", C17),
        ("<stack>", C17),
        ("<stacktrace>", C23),
        ("<stdexcept>", C17),
        ("<stdfloat>", C23),
        ("<stop_token>", C20),
        ("<streambuf>", C17),
        ("<string>", C17),
        ("<string_view>", C17),
        ("<syncstream>", C20),
        ("<system_error>", C17),
        ("<text_encoding>", C26),
        ("<thread>", C17),
        ("<tuple>", C17),
        ("<type_traits>", C17),
        ("<typeindex>", C17),
        ("<typeinfo>", C17),
        ("<unordered_map>", C17),
        ("<unordered_set>", C17),
        ("<utility>", C17),
        ("<valarray>", C17),
        ("<variant>", C17),
        ("<vector>", C17),
        ("<version>", C20),
    ]
};

impl Header {
    /// A lookup in the closed table of the C++ standard library's headers
    /// (C++17 to C++26, the `<cxxx>` forms only), with its angle brackets:
    /// `Header::standard("<vector>")`.
    pub fn standard(name: &str) -> Option<Header> {
        let index = TABLE.binary_search_by(|(header, _)| (*header).cmp(name)).ok()?;
        TABLE.get(index).map(|(header, _)| Header(header))
    }

    /// A header this crate names itself; the tests check it is in the table.
    pub(crate) const fn known(name: &'static str) -> Header {
        Header(name)
    }

    /// The first standard that has it (C++17 for every header that C++17
    /// already had).
    pub fn since(self) -> CppStandard {
        TABLE
            .binary_search_by(|(header, _)| (*header).cmp(self.0))
            .ok()
            .and_then(|index| TABLE.get(index))
            .map_or(CppStandard::Cpp17, |(_, since)| *since)
    }

    /// The header with its angle brackets, as an `#include` writes it.
    pub fn as_str(self) -> &'static str {
        self.0
    }
}

impl fmt::Display for Header {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl Serialize for Header {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.0)
    }
}

impl<'de> Deserialize<'de> for Header {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Header::standard(&name).ok_or_else(|| serde::de::Error::custom("not a standard header"))
    }
}

/// The catalog headers of an item, by where its code is printed (types.md
/// §5.1). Code generation includes `decl` where the declaration is printed
/// and `body` where the definition is, so a body edit never changes a header
/// file (ADR-0019).
///
/// In the SAST JSON it is left out while both sets are empty.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct ItemHeaders {
    /// Blocks printed with the declaration: the signature, default
    /// arguments, a `constexpr` constant's value, default member
    /// initialisers.
    pub decl: BTreeSet<Header>,
    /// Blocks printed only with the definition: function bodies, a
    /// variable's initialiser, a `const` constant's value (ADR-0019 prints it
    /// in the `.cpp`).
    pub body: BTreeSet<Header>,
}

impl ItemHeaders {
    /// Whether neither part has a header.
    pub fn is_empty(&self) -> bool {
        self.decl.is_empty() && self.body.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_sorted_unique_and_bracketed() {
        assert!(
            TABLE.windows(2).all(|pair| pair[0].0 < pair[1].0),
            "sorted and unique"
        );
        for (name, _) in TABLE {
            let inner = name.strip_prefix('<').and_then(|n| n.strip_suffix('>')).unwrap();
            assert!(!inner.is_empty(), "{name}");
            assert!(
                inner
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
                "{name}"
            );
        }
    }

    #[test]
    fn lookups() {
        let vector = Header::standard("<vector>").unwrap();
        assert_eq!(vector.as_str(), "<vector>");
        assert_eq!(vector.to_string(), "<vector>");
        assert_eq!(vector.since(), CppStandard::Cpp17);
        assert_eq!(Header::standard("<compare>").unwrap().since(), CppStandard::Cpp20);
        assert_eq!(Header::standard("<version>").unwrap().since(), CppStandard::Cpp20);
        assert_eq!(Header::standard("<print>").unwrap().since(), CppStandard::Cpp23);
        assert_eq!(
            Header::standard("<inplace_vector>").unwrap().since(),
            CppStandard::Cpp26
        );
        for bad in [
            "vector",
            "<vector",
            "<math.h>",
            "<stdio.h>",
            "<bits/stdc++.h>",
            "<ciso646>",
            "<strstream>",
            "\"vector\"",
            "<Vector>",
            "",
        ] {
            assert_eq!(Header::standard(bad), None, "{bad}");
        }
        assert!(Header::standard("<algorithm>") < Header::standard("<vector>"));
    }

    #[test]
    fn serde_and_item_headers() {
        let string = Header::standard("<string>").unwrap();
        assert_eq!(serde_json::to_string(&string).unwrap(), r#""<string>""#);
        assert_eq!(serde_json::from_str::<Header>(r#""<string>""#).unwrap(), string);
        assert!(serde_json::from_str::<Header>(r#""<evil>""#).is_err());
        let mut headers = ItemHeaders::default();
        assert!(headers.is_empty());
        headers.body.insert(string);
        assert!(!headers.is_empty());
        assert_eq!(
            serde_json::to_value(&headers).unwrap(),
            serde_json::json!({"decl": [], "body": ["<string>"]})
        );
    }
}
