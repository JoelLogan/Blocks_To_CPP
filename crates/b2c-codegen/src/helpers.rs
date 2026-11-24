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
    /// `b2c::read_line(file, line)` (*read line*, *for each line in file*).
    ReadLine,
    /// `b2c::format(pattern, values…)` (*format … with*, C++17 only; C++20
    /// has `std::format`).
    Format,
}

impl Helper {
    /// Every helper, in emission order.
    #[cfg(test)]
    pub(crate) const ALL: [Self; 15] = [
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
        Self::ReadLine,
        Self::Format,
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
            Self::ReadLine => &["<istream>", "<string>"],
            Self::Format => &[
                "<charconv>",
                "<cstddef>",
                "<iomanip>",
                "<ios>",
                "<sstream>",
                "<string>",
            ],
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
            Self::ReadLine => READ_LINE,
            Self::Format => FORMAT,
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

const READ_LINE: &str = "\
/// Reads the next line of `file` into `line`, without its line break, as
/// `std::getline` does, and also without the '\\r' that a file saved with
/// Windows line endings (\"\\r\\n\") leaves at its end on Linux and macOS.
/// Gives back the stream, which converts to `false` when no line was left.
inline std::istream& read_line(std::istream& file, std::string& line) {
\tif (std::getline(file, line) && !line.empty() && line.back() == '\\r') {
\t\tline.pop_back();
\t}
\treturn file;
}
";

const FORMAT: &str = "\
/// One place of a `b2c::format` pattern, read from what follows its `{:`:
/// `[[fill]align][0][width][.precision][letter]`.
struct FormatPlace {
	std::string fill = \" \";
	char align = '\\0';
	bool zero = false;
	std::size_t width = 0;
	int precision = -1;
	char letter = '\\0';
};

/// Reads a place's spec, the text between `{:` and `}`.
inline FormatPlace format_place(const std::string& spec) {
	FormatPlace place;
	const auto is_align = [](char c) { return c == '<' || c == '>' || c == '^'; };
	// The fill is one character, up to four bytes of UTF-8, before an alignment.
	std::size_t fill = 1;
	while (fill < spec.size() && fill < 4 && (static_cast<unsigned char>(spec[fill]) & 0xC0) == 0x80) {
		++fill;
	}
	std::size_t at = 0;
	if (spec.size() > fill && is_align(spec[fill])) {
		place.fill = spec.substr(0, fill);
		place.align = spec[fill];
		at = fill + 1;
	} else if (!spec.empty() && is_align(spec[0])) {
		place.align = spec[0];
		at = 1;
	}
	if (at < spec.size() && spec[at] == '0') {
		place.zero = true;
		++at;
	}
	while (at < spec.size() && spec[at] >= '0' && spec[at] <= '9') {
		place.width = place.width * 10 + static_cast<std::size_t>(spec[at] - '0');
		++at;
	}
	if (at < spec.size() && spec[at] == '.') {
		place.precision = 0;
		++at;
		while (at < spec.size() && spec[at] >= '0' && spec[at] <= '9') {
			place.precision = place.precision * 10 + (spec[at] - '0');
			++at;
		}
	}
	if (at < spec.size()) {
		place.letter = spec[at];
	}
	return place;
}

/// The number of characters of a UTF-8 text: its bytes that do not continue
/// a character.
inline std::size_t format_length(const std::string& text) {
	std::size_t length = 0;
	for (const char byte : text) {
		if ((static_cast<unsigned char>(byte) & 0xC0) != 0x80) {
			++length;
		}
	}
	return length;
}

/// `text` filled to the place's width, on the side its alignment says, or
/// on the side `align` says when the place names none.
inline std::string format_fill(const std::string& text, const FormatPlace& place, char align) {
	const std::size_t length = format_length(text);
	if (length >= place.width) {
		return text;
	}
	const std::size_t missing = place.width - length;
	const char side = place.align == '\\0' ? align : place.align;
	const std::size_t before = side == '>' ? missing : side == '^' ? missing / 2 : std::size_t{0};
	std::string filled;
	for (std::size_t i = 0; i < before; ++i) {
		filled += place.fill;
	}
	filled += text;
	for (std::size_t i = before; i < missing; ++i) {
		filled += place.fill;
	}
	return filled;
}

/// A number's text, filled on the left, or with zeros after its sign for
/// the `0` option when the place names no alignment and the number is
/// finite.
inline std::string format_number(const std::string& text, const FormatPlace& place) {
	const std::size_t sign = !text.empty() && text[0] == '-' ? 1U : 0U;
	const bool digit = text.size() > sign && text[sign] >= '0' && text[sign] <= '9';
	if (place.zero && place.align == '\\0' && digit && text.size() < place.width) {
		std::string padded = text;
		padded.insert(sign, place.width - text.size(), '0');
		return padded;
	}
	return format_fill(text, place, '>');
}

/// A whole number in base 10, or in base 2, 8 or 16 for the letters b, B,
/// o, x and X, with a '-' before a negative number.
inline std::string format_integer(long long value, const FormatPlace& place) {
	const char letter = place.letter;
	const unsigned base = letter == 'b' || letter == 'B' ? 2U
		: letter == 'o' ? 8U
		: letter == 'x' || letter == 'X' ? 16U
		: 10U;
	const std::string digits = letter == 'X' ? \"0123456789ABCDEF\" : \"0123456789abcdef\";
	unsigned long long magnitude = static_cast<unsigned long long>(value);
	if (value < 0) {
		magnitude = 0ULL - magnitude;
	}
	std::string text;
	do {
		text.insert(text.begin(), digits[static_cast<std::size_t>(magnitude % base)]);
		magnitude /= base;
	} while (magnitude > 0);
	if (value < 0) {
		text.insert(text.begin(), '-');
	}
	return format_number(text, place);
}

/// A whole number, or with the letter c the character of that code.
inline std::string format_value(const FormatPlace& place, int value) {
	if (place.letter == 'c') {
		return format_fill(std::string(1, static_cast<char>(value)), place, '<');
	}
	return format_integer(value, place);
}

/// A character, or with b, B, d, o, x or X its code.
inline std::string format_value(const FormatPlace& place, char value) {
	if (place.letter == '\\0' || place.letter == 'c') {
		return format_fill(std::string(1, value), place, '<');
	}
	return format_integer(static_cast<unsigned char>(value), place);
}

/// `true` or `false`, or with b, B, d, o, x or X its number, 1 or 0.
inline std::string format_value(const FormatPlace& place, bool value) {
	if (place.letter == '\\0' || place.letter == 's') {
		return format_fill(value ? \"true\" : \"false\", place, '<');
	}
	return format_integer(value ? 1 : 0, place);
}

/// A decimal number: in the fewest digits that read back as the same
/// number, or as `printf` writes it with the letters f, e and g (F, E and G
/// in capitals) and a precision (6 when the place names none).
inline std::string format_value(const FormatPlace& place, double value) {
	if (place.letter == '\\0' && place.precision < 0) {
		char shortest[32];
		const std::to_chars_result end = std::to_chars(shortest, shortest + sizeof shortest, value);
		return format_number(std::string(shortest, end.ptr), place);
	}
	std::ostringstream text;
	const char letter = place.letter;
	if (letter == 'f' || letter == 'F') {
		text << std::fixed;
	} else if (letter == 'e' || letter == 'E') {
		text << std::scientific;
	}
	if (letter == 'F' || letter == 'E' || letter == 'G') {
		text << std::uppercase;
	}
	text << std::setprecision(place.precision < 0 ? 6 : place.precision) << value;
	return format_number(text.str(), place);
}

/// A text, or with a precision its first characters.
inline std::string format_value(const FormatPlace& place, const std::string& value) {
	std::string text = value;
	if (place.precision >= 0) {
		std::size_t characters = 0;
		for (std::size_t at = 0; at < text.size(); ++at) {
			const bool starts = (static_cast<unsigned char>(text[at]) & 0xC0) != 0x80;
			if (starts && characters++ == static_cast<std::size_t>(place.precision)) {
				text.erase(at);
				break;
			}
		}
	}
	return format_fill(text, place, '<');
}

/// A text written in the program, `\"...\"`.
inline std::string format_value(const FormatPlace& place, const char* value) {
	return format_value(place, std::string(value));
}

/// Copies `pattern` from `at` into `out` up to its next place, with `{{` and
/// `}}` as single braces, then reads that place's spec (what follows its
/// `:`) into `spec` and moves `at` past it. Gives back false at the end of
/// the pattern.
inline bool format_next(std::ostringstream& out, const std::string& pattern, std::size_t& at, std::string& spec) {
	while (at < pattern.size()) {
		const char c = pattern[at];
		if ((c == '{' || c == '}') && at + 1 < pattern.size() && pattern[at + 1] == c) {
			out << c;
			at += 2;
		} else if (c == '{') {
			const std::size_t close = pattern.find('}', at);
			spec = pattern.substr(at + 1, close - at - 1);
			if (!spec.empty()) {
				spec.erase(0, 1);
			}
			at = close + 1;
			return true;
		} else {
			out << c;
			++at;
		}
	}
	return false;
}

/// The rest of the pattern, after its last place.
inline void format_rest(std::ostringstream& out, const std::string& pattern, std::size_t at) {
	std::string spec;
	static_cast<void>(format_next(out, pattern, at, spec));
}

/// The rest of the pattern, with `value` in its next place and `rest` in
/// the places after it.
template <typename T, typename... Rest>
void format_rest(std::ostringstream& out, const std::string& pattern, std::size_t at, const T& value, const Rest&... rest) {
	std::string spec;
	if (format_next(out, pattern, at, spec)) {
		out << format_value(format_place(spec), value);
	}
	format_rest(out, pattern, at, rest...);
}

/// Text from a pattern, as the format function of C++20 writes it: each
/// `{}` or `{:spec}` is replaced by the next value, and `{{` and `}}` are
/// braces of their own. The block checks the pattern: as many places as
/// values, each spec `[[fill]align][0][width][.precision][letter]` fitting
/// its value.
template <typename... Values>
std::string format(const std::string& pattern, const Values&... values) {
	std::ostringstream out;
	format_rest(out, pattern, 0, values...);
	return out.str();
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
