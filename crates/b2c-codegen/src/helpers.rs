//! The support helpers in namespace `b2c` (spec §3.9).
//!
//! Helpers are plain, readable C++ with Doxygen comments, emitted only when
//! used. Their text is fixed (it never contains user text). Each leading tab
//! stands for one indentation level, so the helpers follow the configured
//! indentation width.
//!
//! The namespace name `b2c` cannot clash with user code: user identifiers may
//! not start with `b2c` in any letter case (spec §8.4.1, `Ident::new`).

/// A support helper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Helper {
    /// `b2c::random_int(low, high)`.
    RandomInt,
    /// `b2c::ask_line(prompt)`: reads a line of text.
    AskLine,
    /// `b2c::ask<T>(prompt)`: reads a number, character or true/false value.
    Ask,
    /// `b2c::to_text(x)`: a decimal number as text, written as `print` writes it.
    ToText,
    /// `b2c::split(text, separator)` (*split … by*; w2.md §2.12).
    Split,
    /// `b2c::to_lower(text)` (*lowercase*).
    ToLower,
    /// `b2c::to_upper(text)` (*uppercase*).
    ToUpper,
    /// `b2c::trim(text)` (*trim*).
    Trim,
    /// `b2c::replace_all(text, old, new)` (*replace all*).
    ReplaceAll,
    /// `b2c::starts_with(text, prefix)` (*starts with*, C++17 only).
    StartsWith,
    /// `b2c::ends_with(text, suffix)` (*ends with*, C++17 only).
    EndsWith,
    /// `b2c::random_real(low, high)` (*random decimal*).
    RandomReal,
    /// `b2c::remove_at(items, index)` (*remove item at*).
    RemoveAt,
}

impl Helper {
    /// Every helper, in emission order.
    #[cfg(test)]
    pub(crate) const ALL: [Self; 13] = [
        Self::RandomInt,
        Self::AskLine,
        Self::Ask,
        Self::ToText,
        Self::Split,
        Self::ToLower,
        Self::ToUpper,
        Self::Trim,
        Self::ReplaceAll,
        Self::StartsWith,
        Self::EndsWith,
        Self::RandomReal,
        Self::RemoveAt,
    ];

    /// The standard headers the helper's code needs.
    pub(crate) fn headers(self) -> &'static [&'static str] {
        match self {
            Self::RandomInt | Self::RandomReal => &["<random>", "<utility>"],
            Self::AskLine => &["<cstdlib>", "<iostream>", "<limits>", "<string>"],
            Self::Ask => &[
                "<cctype>",
                "<cstdlib>",
                "<iostream>",
                "<limits>",
                "<sstream>",
                "<string>",
            ],
            Self::ToText => &["<iostream>", "<sstream>", "<string>"],
            Self::Split => &["<cstddef>", "<string>", "<vector>"],
            Self::ToLower | Self::ToUpper | Self::StartsWith | Self::EndsWith => &["<string>"],
            Self::Trim | Self::ReplaceAll => &["<cstddef>", "<string>"],
            Self::RemoveAt => &["<cstddef>"],
        }
    }

    /// The helper's C++ code (tab-indented, ending with a line break).
    pub(crate) fn source(self) -> &'static str {
        match self {
            Self::RandomInt => RANDOM_INT,
            Self::AskLine => ASK_LINE,
            Self::Ask => ASK,
            Self::ToText => TO_TEXT,
            Self::Split => SPLIT,
            Self::ToLower => TO_LOWER,
            Self::ToUpper => TO_UPPER,
            Self::Trim => TRIM,
            Self::ReplaceAll => REPLACE_ALL,
            Self::StartsWith => STARTS_WITH,
            Self::EndsWith => ENDS_WITH,
            Self::RandomReal => RANDOM_REAL,
            Self::RemoveAt => REMOVE_AT,
        }
    }
}

/// The marker line that opens the inline helper section.
pub(crate) const SECTION_START: &str =
    "// ---- Blocks2Cpp support helpers (only those used) ------------------------";

/// The marker line that closes the inline helper section.
pub(crate) const SECTION_END: &str =
    "// ---- End of Blocks2Cpp support helpers -----------------------------------";

const RANDOM_INT: &str = "\
/// Returns a random whole number between `low` and `high`, including both.
/// The bounds may be given in either order. The random engine is seeded once,
/// from `std::random_device`, the first time this function is called.
inline int random_int(int low, int high) {
\tstatic std::mt19937 engine{std::random_device{}()};
\tif (low > high) {
\t\tstd::swap(low, high);
\t}
\tstd::uniform_int_distribution<int> distribution(low, high);
\treturn distribution(engine);
}
";

const ASK_LINE: &str = "\
/// Prints `prompt` and reads one line of text, without its leading spaces.
/// An empty line asks again. At the end of the input (for example when the
/// input comes from a file), prints \"Input ended\" to the error stream and
/// exits with code 1. If an earlier plain `std::cin >>` read failed, its
/// line is dropped first, so the answer is read from the next line.
inline std::string ask_line(const std::string& prompt = \"\") {
\tif (std::cin.fail() && !std::cin.eof()) {
\t\tstd::cin.clear();
\t\tstd::cin.ignore(std::numeric_limits<std::streamsize>::max(), '\\n');
\t}
\twhile (true) {
\t\tstd::cout << prompt;
\t\tstd::string line;
\t\tif (!std::getline(std::cin, line)) {
\t\t\tstd::cerr << \"Input ended\" << '\\n';
\t\t\tstd::exit(1);
\t\t}
\t\tif (!line.empty() && line.back() == '\\r') {
\t\t\tline.pop_back();
\t\t}
\t\tconst auto start = line.find_first_not_of(\" \\t\");
\t\tif (start != std::string::npos) {
\t\t\treturn line.substr(start);
\t\t}
\t}
}
";

const ASK: &str = "\
/// The message shown when the typed input is not a valid `T`.
template <typename T>
const char* invalid_input_message() {
\treturn \"Please enter a valid value.\";
}

/// The message shown when the typed input is not a whole number.
template <>
inline const char* invalid_input_message<int>() {
\treturn \"Please enter a whole number.\";
}

/// The message shown when the typed input is not a number.
template <>
inline const char* invalid_input_message<double>() {
\treturn \"Please enter a number.\";
}

/// The message shown when the typed input is not one character.
template <>
inline const char* invalid_input_message<char>() {
\treturn \"Please enter one character.\";
}

/// The message shown when the typed input is not true or false.
template <>
inline const char* invalid_input_message<bool>() {
\treturn \"Please enter true or false.\";
}

/// Reads one `T` from `in`. Returns false if the text there does not start
/// with a valid `T`.
template <typename T>
bool read_value(std::istream& in, T& value) {
\treturn static_cast<bool>(in >> value);
}

/// Reads a true/false answer from `in`: accepts true/false, yes/no and 1/0,
/// in any letter case. Returns false for any other word.
inline bool read_value(std::istream& in, bool& value) {
\tstd::string word;
\tif (!(in >> word)) {
\t\treturn false;
\t}
\tfor (char& letter : word) {
\t\tletter = static_cast<char>(std::tolower(static_cast<unsigned char>(letter)));
\t}
\tif (word == \"true\" || word == \"yes\" || word == \"1\") {
\t\tvalue = true;
\t\treturn true;
\t}
\tif (word == \"false\" || word == \"no\" || word == \"0\") {
\t\tvalue = false;
\t\treturn true;
\t}
\treturn false;
}

/// Prints `prompt` and reads one line holding a `T` (a number, a character
/// or true/false), asking again until the line is valid: apart from spaces,
/// the whole line must be one `T`, so `3.5` or `12abc` is not a whole
/// number. At the end of the input (for example when the input comes from a
/// file), prints \"Input ended\" to the error stream and exits with code 1
/// instead of asking forever. If an earlier plain `std::cin >>` read failed,
/// its line is dropped first, so the answer is read from the next line.
template <typename T>
T ask(const std::string& prompt = \"\") {
\tif (std::cin.fail() && !std::cin.eof()) {
\t\tstd::cin.clear();
\t\tstd::cin.ignore(std::numeric_limits<std::streamsize>::max(), '\\n');
\t}
\twhile (true) {
\t\tstd::cout << prompt;
\t\tstd::string line;
\t\tif (!std::getline(std::cin, line)) {
\t\t\tstd::cerr << \"Input ended\" << '\\n';
\t\t\tstd::exit(1);
\t\t}
\t\tstd::istringstream in(line);
\t\tT value{};
\t\tif (read_value(in, value) && (in >> std::ws).eof()) {
\t\t\treturn value;
\t\t}
\t\tstd::cout << invalid_input_message<T>() << '\\n';
\t}
}
";

const TO_TEXT: &str = "\
/// Returns `value` as text, written exactly as `std::cout << value` would
/// write it now (2.5 as \"2.5\", where `std::to_string` gives \"2.500000\").
inline std::string to_text(double value) {
\tstd::ostringstream out;
\tout.copyfmt(std::cout);
\tout << value;
\treturn out.str();
}
";

const SPLIT: &str = "\
/// Splits `text` at every `separator` and returns the parts in order. Two
/// separators in a row give an empty part between them, and so does a
/// separator at either end. An empty separator gives one part per character.
inline std::vector<std::string> split(const std::string& text, const std::string& separator) {
\tstd::vector<std::string> parts;
\tif (separator.empty()) {
\t\tfor (const char letter : text) {
\t\t\tparts.push_back(std::string(1, letter));
\t\t}
\t\treturn parts;
\t}
\tstd::size_t start = 0;
\twhile (true) {
\t\tconst std::size_t found = text.find(separator, start);
\t\tif (found == std::string::npos) {
\t\t\tparts.push_back(text.substr(start));
\t\t\treturn parts;
\t\t}
\t\tparts.push_back(text.substr(start, found - start));
\t\tstart = found + separator.size();
\t}
}
";

const TO_LOWER: &str = "\
/// Returns `text` with the letters A to Z in lower case. Every other
/// character, also a letter outside ASCII, is left as it is.
inline std::string to_lower(std::string text) {
\tfor (char& letter : text) {
\t\tif (letter >= 'A' && letter <= 'Z') {
\t\t\tletter = static_cast<char>(letter - 'A' + 'a');
\t\t}
\t}
\treturn text;
}
";

const TO_UPPER: &str = "\
/// Returns `text` with the letters a to z in upper case. Every other
/// character, also a letter outside ASCII, is left as it is.
inline std::string to_upper(std::string text) {
\tfor (char& letter : text) {
\t\tif (letter >= 'a' && letter <= 'z') {
\t\t\tletter = static_cast<char>(letter - 'a' + 'A');
\t\t}
\t}
\treturn text;
}
";

const TRIM: &str = "\
/// Returns `text` without the spaces and tabs at its start and its end.
inline std::string trim(const std::string& text) {
\tconst std::size_t first = text.find_first_not_of(\" \\t\");
\tif (first == std::string::npos) {
\t\treturn std::string();
\t}
\tconst std::size_t last = text.find_last_not_of(\" \\t\");
\treturn text.substr(first, last - first + 1);
}
";

const REPLACE_ALL: &str = "\
/// Returns `text` with every `old_text` replaced by `new_text`, from left to
/// right; what a replacement puts in is never replaced again. An empty
/// `old_text` changes nothing.
inline std::string replace_all(std::string text, const std::string& old_text, const std::string& new_text) {
\tif (old_text.empty()) {
\t\treturn text;
\t}
\tstd::size_t at = text.find(old_text);
\twhile (at != std::string::npos) {
\t\ttext.replace(at, old_text.size(), new_text);
\t\tat = text.find(old_text, at + new_text.size());
\t}
\treturn text;
}
";

const STARTS_WITH: &str = "\
/// Whether `text` starts with `prefix` (C++20's `text.starts_with(prefix)`).
inline bool starts_with(const std::string& text, const std::string& prefix) {
\treturn text.size() >= prefix.size() && text.compare(0, prefix.size(), prefix) == 0;
}
";

const ENDS_WITH: &str = "\
/// Whether `text` ends with `suffix` (C++20's `text.ends_with(suffix)`).
inline bool ends_with(const std::string& text, const std::string& suffix) {
\treturn text.size() >= suffix.size()
\t\t&& text.compare(text.size() - suffix.size(), suffix.size(), suffix) == 0;
}
";

const RANDOM_REAL: &str = "\
/// Returns a random decimal number from `low` up to, but not including,
/// `high` (`low` itself when both are equal). The bounds may be given in
/// either order. The random engine is seeded once, from
/// `std::random_device`, the first time this function is called.
inline double random_real(double low, double high) {
\tstatic std::mt19937 engine{std::random_device{}()};
\tif (low > high) {
\t\tstd::swap(low, high);
\t}
\tstd::uniform_real_distribution<double> distribution(low, high);
\treturn distribution(engine);
}
";

const REMOVE_AT: &str = "\
/// Removes the item at position `index` of a list or a deque; the items
/// after it move one place forward. As with `.at()`, a position the list
/// does not have stops the program with an out-of-range error.
template <typename C>
void remove_at(C& items, int index) {
\tstatic_cast<void>(items.at(static_cast<std::size_t>(index)));
\titems.erase(items.begin() + index);
}
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helper_text_is_well_formed() {
        for helper in Helper::ALL {
            let source = helper.source();
            assert!(source.ends_with("}\n"), "{helper:?}");
            for line in source.lines() {
                assert_eq!(line, line.trim_end(), "trailing whitespace in {helper:?}");
                assert!(
                    !line.trim_start_matches('\t').starts_with([' ', '\t']),
                    "{helper:?}: {line}"
                );
            }
            let mut headers = helper.headers().to_vec();
            headers.sort_unstable();
            headers.dedup();
            assert_eq!(
                headers,
                helper.headers(),
                "{helper:?} headers must be sorted and unique"
            );
        }
        assert_eq!(SECTION_START.len(), SECTION_END.len());
    }

    #[test]
    fn helper_namespace_cannot_clash_with_user_names() {
        use b2c_ir::text::Ident;
        assert!(Ident::generated("b2c").is_ok());
        for name in ["b2c", "B2C", "b2c_random_int", "B2c_x"] {
            assert!(Ident::new(name).is_err(), "{name}");
        }
    }
}
