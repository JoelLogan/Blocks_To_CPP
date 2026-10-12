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
}

impl Helper {
    /// Every helper, in emission order.
    #[cfg(test)]
    pub(crate) const ALL: [Self; 4] = [Self::RandomInt, Self::AskLine, Self::Ask, Self::ToText];

    /// The standard headers the helper's code needs.
    pub(crate) fn headers(self) -> &'static [&'static str] {
        match self {
            Self::RandomInt => &["<random>", "<utility>"],
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
        }
    }

    /// The helper's C++ code (tab-indented, ending with a line break).
    pub(crate) fn source(self) -> &'static str {
        match self {
            Self::RandomInt => RANDOM_INT,
            Self::AskLine => ASK_LINE,
            Self::Ask => ASK,
            Self::ToText => TO_TEXT,
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
