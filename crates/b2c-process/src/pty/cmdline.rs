//! Windows command lines and environment blocks, built from UTF-16 code
//! units. Pure functions, so they are tested on every platform.
//!
//! `CreateProcessW` takes one command-line string that the program splits
//! into `argv` itself. Programs built with MinGW or MSVC split it with the
//! Microsoft C runtime rules (the same as `CommandLineToArgvW`), which
//! [`command_line`] inverts exactly: every argument reaches the program as
//! one `argv` entry, unchanged, and no shell is involved. Only `.exe`
//! programs are ever started (`Command::new` refuses anything else), so the
//! different rules of `cmd.exe` for batch files never apply.

/// The longest command line `CreateProcessW` accepts, in UTF-16 code units
/// including the terminating NUL.
pub(crate) const MAX_COMMAND_LINE: usize = 32_767;

const NUL: u16 = 0;
const QUOTE: u16 = b'"' as u16;
const BACKSLASH: u16 = b'\\' as u16;
const SPACE: u16 = b' ' as u16;
const TAB: u16 = b'\t' as u16;
const EQUALS: u16 = b'=' as u16;

/// Builds the NUL-terminated command line for `program` (`argv[0]`) and
/// `args`.
///
/// `argv[0]` is always quoted, without escapes (the C runtime reads it up to
/// the next quote), so it must not contain `"`; Windows paths cannot. Each
/// other argument is quoted when it is empty or contains a space or tab,
/// and backslashes are doubled only where they precede a quote.
///
/// # Errors
/// A description when a part contains NUL, `program` contains `"`, or the
/// result is longer than [`MAX_COMMAND_LINE`].
pub(crate) fn command_line(program: &[u16], args: &[Vec<u16>]) -> Result<Vec<u16>, &'static str> {
    if program.contains(&NUL) {
        return Err("the program path contains a NUL character");
    }
    if program.contains(&QUOTE) {
        return Err("the program path contains a quotation mark");
    }
    let mut line = Vec::with_capacity(program.len() + 3);
    line.push(QUOTE);
    line.extend_from_slice(program);
    line.push(QUOTE);
    for arg in args {
        if arg.contains(&NUL) {
            return Err("an argument contains a NUL character");
        }
        line.push(SPACE);
        append_arg(&mut line, arg);
        if line.len() >= MAX_COMMAND_LINE {
            return Err("the command line is longer than 32,767 characters");
        }
    }
    line.push(NUL);
    if line.len() > MAX_COMMAND_LINE {
        return Err("the command line is longer than 32,767 characters");
    }
    Ok(line)
}

/// Appends one argument, quoted and escaped so that the C runtime reads it
/// back unchanged.
fn append_arg(line: &mut Vec<u16>, arg: &[u16]) {
    let quote = arg.is_empty() || arg.iter().any(|&unit| unit == SPACE || unit == TAB);
    if quote {
        line.push(QUOTE);
    }
    let mut backslashes = 0_usize;
    for &unit in arg {
        if unit == BACKSLASH {
            backslashes += 1;
        } else {
            if unit == QUOTE {
                // Backslashes before a quote are halved by the reader, and
                // the quote itself needs one more to be literal.
                line.extend(std::iter::repeat_n(BACKSLASH, backslashes + 1));
            }
            backslashes = 0;
        }
        line.push(unit);
    }
    if quote {
        // Backslashes before the closing quote are halved too.
        line.extend(std::iter::repeat_n(BACKSLASH, backslashes));
        line.push(QUOTE);
    }
}

/// Builds a `CREATE_UNICODE_ENVIRONMENT` block: `NAME=value` strings, each
/// NUL-terminated, sorted by name without regard to ASCII case (as Windows
/// keeps its own blocks), and a final NUL. An empty environment is two NULs.
///
/// # Errors
/// A description when a name is empty, contains `=` after its first
/// character (names such as `=C:` are valid) or NUL, or a value contains
/// NUL.
pub(crate) fn environment_block(mut vars: Vec<(Vec<u16>, Vec<u16>)>) -> Result<Vec<u16>, &'static str> {
    for (name, value) in &vars {
        if name.is_empty() {
            return Err("an environment variable has an empty name");
        }
        if name.contains(&NUL) || value.contains(&NUL) {
            return Err("an environment variable contains a NUL character");
        }
        if name.iter().skip(1).any(|&unit| unit == EQUALS) {
            return Err("an environment variable name contains '='");
        }
    }
    vars.sort_by(|(a, _), (b, _)| {
        a.iter()
            .map(|&unit| fold_ascii(unit))
            .cmp(b.iter().map(|&unit| fold_ascii(unit)))
    });
    let mut block = Vec::new();
    if vars.is_empty() {
        block.push(NUL);
    }
    for (name, value) in vars {
        block.extend_from_slice(&name);
        block.push(EQUALS);
        block.extend_from_slice(&value);
        block.push(NUL);
    }
    block.push(NUL);
    Ok(block)
}

/// `text` followed by a NUL, for a path parameter.
///
/// # Errors
/// A description when `text` contains NUL itself.
pub(crate) fn nul_terminated(text: &[u16], what: &'static str) -> Result<Vec<u16>, &'static str> {
    if text.contains(&NUL) {
        return Err(what);
    }
    let mut terminated = Vec::with_capacity(text.len() + 1);
    terminated.extend_from_slice(text);
    terminated.push(NUL);
    Ok(terminated)
}

/// Upper-cases ASCII letters, leaving every other code unit as it is.
fn fold_ascii(unit: u16) -> u16 {
    if (u16::from(b'a')..=u16::from(b'z')).contains(&unit) {
        unit - 32
    } else {
        unit
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    fn narrow(units: &[u16]) -> String {
        String::from_utf16_lossy(units)
    }

    /// Splits a command line the way the Microsoft C runtime does (and
    /// `CommandLineToArgvW` since 2008): the reference [`command_line`] must
    /// invert.
    fn parse(line: &[u16]) -> Vec<Vec<u16>> {
        let line = line.strip_suffix(&[NUL]).unwrap_or(line);
        let mut args = Vec::new();
        let mut index = 0;
        // argv[0]: quoted up to the next quote, or up to whitespace.
        let mut program = Vec::new();
        if line.first() == Some(&QUOTE) {
            index = 1;
            while index < line.len() && line[index] != QUOTE {
                program.push(line[index]);
                index += 1;
            }
            index += 1;
        } else {
            while index < line.len() && line[index] != SPACE && line[index] != TAB {
                program.push(line[index]);
                index += 1;
            }
        }
        args.push(program);
        loop {
            while index < line.len() && (line[index] == SPACE || line[index] == TAB) {
                index += 1;
            }
            if index >= line.len() {
                break;
            }
            let mut arg = Vec::new();
            let mut quoted = false;
            while index < line.len() {
                let unit = line[index];
                if !quoted && (unit == SPACE || unit == TAB) {
                    break;
                }
                if unit == BACKSLASH {
                    let start = index;
                    while index < line.len() && line[index] == BACKSLASH {
                        index += 1;
                    }
                    let count = index - start;
                    if index < line.len() && line[index] == QUOTE {
                        arg.extend(std::iter::repeat_n(BACKSLASH, count / 2));
                        if count % 2 == 1 {
                            arg.push(QUOTE);
                            index += 1;
                        }
                    } else {
                        arg.extend(std::iter::repeat_n(BACKSLASH, count));
                    }
                    continue;
                }
                if unit == QUOTE {
                    if quoted && line.get(index + 1) == Some(&QUOTE) {
                        // `""` inside quotes is a literal quote.
                        arg.push(QUOTE);
                        index += 2;
                        continue;
                    }
                    quoted = !quoted;
                    index += 1;
                    continue;
                }
                arg.push(unit);
                index += 1;
            }
            args.push(arg);
        }
        args
    }

    fn line_for(program: &str, args: &[&str]) -> String {
        let args: Vec<Vec<u16>> = args.iter().map(|arg| wide(arg)).collect();
        let line = command_line(&wide(program), &args).unwrap();
        assert_eq!(line.last(), Some(&NUL));
        narrow(&line[..line.len() - 1])
    }

    #[test]
    fn known_command_lines() {
        let program = r"C:\Program Files\app\game.exe";
        assert_eq!(line_for(program, &[]), r#""C:\Program Files\app\game.exe""#);
        assert_eq!(
            line_for(program, &["plain", "two words", "", "tab\there"]),
            "\"C:\\Program Files\\app\\game.exe\" plain \"two words\" \"\" \"tab\there\""
        );
        assert_eq!(
            line_for(program, &[r#"say "hi""#]),
            r#""C:\Program Files\app\game.exe" "say \"hi\"""#
        );
        assert_eq!(
            line_for(program, &[r"C:\dir\", r"C:\my dir\", r#"a\"b"#, r"a\\b"]),
            r#""C:\Program Files\app\game.exe" C:\dir\ "C:\my dir\\" a\\\"b a\\b"#
        );
        // Nothing a shell would interpret is special here.
        assert_eq!(
            line_for(program, &["%PATH%", "a&b", "|", "^", "<x>"]),
            r#""C:\Program Files\app\game.exe" %PATH% a&b | ^ <x>"#
        );
    }

    #[test]
    fn known_lines_parse_back() {
        let args = ["", "a b", r"C:\x\", "\"", r"\\", r#"\""#, "é ✓ 𝄞"];
        let program = wide(r"C:\a\b.exe");
        let wide_args: Vec<Vec<u16>> = args.iter().map(|arg| wide(arg)).collect();
        let parsed = parse(&command_line(&program, &wide_args).unwrap());
        assert_eq!(parsed[0], program);
        assert_eq!(&parsed[1..], wide_args.as_slice());
    }

    #[test]
    fn bad_parts_are_refused() {
        let program = wide(r"C:\a.exe");
        assert!(command_line(&wide("C:\\a\0.exe"), &[]).is_err());
        assert!(command_line(&wide("C:\\\"a.exe"), &[]).is_err());
        assert!(command_line(&program, &[wide("a\0b")]).is_err());
        let long = vec![u16::from(b'x'); MAX_COMMAND_LINE];
        assert!(command_line(&program, &[long]).is_err());
        // Two quotes, a space and the final NUL around the program and the
        // argument.
        let fits = vec![u16::from(b'x'); MAX_COMMAND_LINE - program.len() - 4];
        let line = command_line(&program, &[fits]).unwrap();
        assert_eq!(line.len(), MAX_COMMAND_LINE);
        let too_long = vec![u16::from(b'x'); MAX_COMMAND_LINE - program.len() - 3];
        assert!(command_line(&program, &[too_long]).is_err());
    }

    #[test]
    fn environment_blocks() {
        assert_eq!(environment_block(Vec::new()).unwrap(), vec![0, 0]);
        let block = environment_block(vec![
            (wide("b"), wide("2")),
            (wide("Path"), wide(r"C:\x;C:\y")),
            (wide("A"), wide("")),
            (wide("=C:"), wide(r"C:\work")),
            (wide("SystemRoot"), wide(r"C:\Windows")),
        ])
        .unwrap();
        assert_eq!(
            narrow(&block),
            "=C:=C:\\work\0A=\0b=2\0Path=C:\\x;C:\\y\0SystemRoot=C:\\Windows\0\0"
        );
    }

    #[test]
    fn bad_environments_are_refused() {
        for (name, value) in [("", "x"), ("A=B", "x"), ("A\0", "x"), ("A", "x\0y")] {
            assert!(
                environment_block(vec![(wide(name), wide(value))]).is_err(),
                "{name:?}={value:?}"
            );
        }
    }

    #[test]
    fn nul_terminated_paths() {
        assert_eq!(nul_terminated(&wide("C:\\x"), "bad").unwrap(), wide("C:\\x\0"));
        assert_eq!(nul_terminated(&wide("C:\\x\0y"), "bad"), Err("bad"));
    }

    fn units() -> impl Strategy<Value = Vec<u16>> {
        // Biased towards the characters the quoting rules care about, plus
        // any other non-NUL code unit (lone surrogates included).
        let special = prop_oneof![
            Just(SPACE),
            Just(TAB),
            Just(QUOTE),
            Just(BACKSLASH),
            Just(u16::from(b'a')),
        ];
        let unit = prop_oneof![3 => special, 1 => 1_u16..=u16::MAX];
        proptest::collection::vec(unit, 0..24)
    }

    proptest! {
        #[test]
        fn every_argv_round_trips(args in proptest::collection::vec(units(), 0..8)) {
            let program = wide(r"C:\Program Files\b2c\game.exe");
            let line = command_line(&program, &args).unwrap();
            let parsed = parse(&line);
            prop_assert_eq!(&parsed[0], &program);
            prop_assert_eq!(&parsed[1..], args.as_slice());
        }

        #[test]
        fn environment_blocks_are_sorted_and_terminated(
            vars in proptest::collection::btree_map("[A-Za-z_][A-Za-z0-9_]{0,8}", "[ -~]{0,8}", 0..8)
        ) {
            let pairs: Vec<(Vec<u16>, Vec<u16>)> = vars.iter().map(|(name, value)| (wide(name), wide(value))).collect();
            let block = environment_block(pairs).unwrap();
            prop_assert!(block.ends_with(&[NUL, NUL]));
            let text = narrow(&block[..block.len() - 1]);
            let entries: Vec<&str> = text.split('\0').filter(|entry| !entry.is_empty()).collect();
            prop_assert_eq!(entries.len(), vars.len());
            let names: Vec<String> = entries
                .iter()
                .map(|entry| entry.split_once('=').map_or("", |(name, _)| name).to_ascii_uppercase())
                .collect();
            let mut sorted = names.clone();
            sorted.sort();
            prop_assert_eq!(names, sorted);
        }
    }
}
