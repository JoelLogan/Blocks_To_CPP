//! Encoder round trips (spec §8.4.2, §8.4.3).
//!
//! * String literals: random Unicode text printed by a generated program comes
//!   out byte for byte (compiled with g++), and a pure-Rust decoder of the
//!   emitted C++ literal gives the text back (no g++ needed).
//! * Comments: random comment text, including line terminators, trailing
//!   backslashes and `*/`, never changes what a program does, and only ever
//!   adds `//` lines.

use b2c_codegen::generate;
use b2c_ir::sast::{CompoundOp, OutputStream, PassMode, PrintSeparator, Program, StmtKind};
use b2c_ir::types::Type;
use proptest::prelude::*;
use proptest::strategy::ValueTree;
use proptest::test_runner::TestRunner;

use crate::builder::{Builder, commented, commented_item};
use crate::{generate_checked, gxx, options};

/// Flags that make the source and execution character sets explicit.
const CHARSET_FLAGS: &[&str] = &["-finput-charset=UTF-8", "-fexec-charset=UTF-8"];

/// Characters and snippets that are interesting to encoders.
const TRICKY: &[&str] = &[
    "\\",
    "\\ ",
    " \\",
    "\\\t",
    "\"",
    "'",
    "?",
    "??/",
    "??=",
    "??'",
    "\n",
    "\r",
    "\r\n",
    "\t",
    "\u{1}",
    "\u{1}23",
    "\u{7f}",
    "\u{85}",
    "\u{0B}",
    "\u{0C}",
    "\u{2028}",
    "\u{2029}",
    "\u{202E}",
    "\u{2066}",
    "\u{200B}",
    "\u{FEFF}",
    "\u{AD}",
    "\u{E0041}",
    "\u{10FFFF}",
    "\u{FFFE}",
    "é",
    "✓",
    "😀",
    "\\x41",
    "*/",
    "/*",
    "//",
    "count = 0;",
    "\"); x(); //",
    "0",
];

/// Random text for string literals (no NUL, which literals reject).
fn literal_text() -> impl Strategy<Value = String> {
    let piece = prop_oneof![
        3 => any::<char>().prop_filter("no NUL", |c| *c != '\0').prop_map(String::from),
        2 => prop::sample::select(TRICKY).prop_map(String::from),
        1 => "[ -~]{1,8}",
    ];
    prop::collection::vec(piece, 0..20).prop_map(|parts| parts.concat())
}

/// Random text for comments (anything, NUL included).
fn comment_text() -> impl Strategy<Value = String> {
    let piece = prop_oneof![
        3 => any::<char>().prop_map(String::from),
        3 => prop::sample::select(TRICKY).prop_map(String::from),
        1 => "[ -~]{1,8}",
    ];
    prop::collection::vec(piece, 0..20).prop_map(|parts| parts.concat())
}

/// Draws `count` values from a strategy with a fixed seed.
fn sample<T: std::fmt::Debug>(strategy: impl Strategy<Value = T>, count: usize) -> Vec<T> {
    let mut runner = TestRunner::deterministic();
    (0..count)
        .map(|_| strategy.new_tree(&mut runner).unwrap().current())
        .collect()
}

/// A program printing `<byte length>:<text>` and a line break for each text.
fn print_all(texts: &[String]) -> Program {
    let b = Builder::new();
    let body = texts
        .iter()
        .map(|text| {
            b.print_with(
                vec![b.int(&text.len().to_string()), b.chr(":"), b.str(text)],
                PrintSeparator::None,
                true,
                OutputStream::Out,
            )
        })
        .collect();
    let main = b.main(body);
    b.program(vec![main])
}

#[test]
fn string_literals_print_exactly_their_text() {
    if !gxx::available() {
        return;
    }
    let mut texts = sample(literal_text(), 400);
    texts.extend(TRICKY.iter().map(|t| (*t).to_owned()));
    texts.push(TRICKY.concat());
    let project = generate_checked(&print_all(&texts), &options("Literal Round Trip"));
    let executable = gxx::build(&project, CHARSET_FLAGS).unwrap();
    let out = gxx::run(&executable, b"");
    assert!(out.status.success());
    let mut rest = out.stdout.as_slice();
    for text in &texts {
        let expected = format!("{}:{text}\n", text.len());
        assert!(
            rest.starts_with(expected.as_bytes()),
            "{text:?} did not round-trip; output was {:?}",
            String::from_utf8_lossy(&rest[..rest.len().min(expected.len() + 20)])
        );
        rest = &rest[expected.len()..];
    }
    assert!(rest.is_empty());
}

/// A program whose output is fixed (`<n>` and `bumped`), with the given
/// comments on its items and statements, some nested.
fn counting_program(comments: &[String]) -> (Program, String) {
    let b = Builder::new();
    let bump = b.function("bump", Type::Void, &[("value", Type::Int, PassMode::Editable)]);
    let value = bump.params[0].clone();
    let mut bump_body = vec![b.stmt(StmtKind::CompoundAssign {
        target: value,
        op: CompoundOp::Add,
        value: b.int("1"),
    })];
    let count = b.var("count", Type::Int);
    let mut body = vec![b.declare(&count, Some(b.int("0")))];
    let increment = || {
        b.stmt(StmtKind::CompoundAssign {
            target: count.clone(),
            op: CompoundOp::Add,
            value: b.int("1"),
        })
    };
    let mut expected = 0;
    for (index, comment) in comments.iter().enumerate() {
        match index % 4 {
            0 => body.push(commented(increment(), comment)),
            1 => body.push(commented(b.eval(b.call(&bump, vec![b.get(&count)])), comment)),
            2 => body.push(b.if_else(
                vec![(b.boolean(true), vec![commented(increment(), comment)])],
                None,
            )),
            _ => {
                bump_body.push(commented(b.print_text("bumped"), comment));
                continue;
            }
        }
        expected += 1;
    }
    body.push(b.print(vec![b.get(&count)]));
    let calls = comments.iter().enumerate().filter(|(i, _)| i % 4 == 1).count();
    let lines_per_call = comments.iter().enumerate().filter(|(i, _)| i % 4 == 3).count();
    let main = b.main(body);
    let main = match comments.first() {
        Some(comment) => commented_item(main, comment),
        None => main,
    };
    let bump_def = b.define(&bump, bump_body);
    let bump_def = match comments.last() {
        Some(comment) => commented_item(bump_def, comment),
        None => bump_def,
    };
    let output = format!("{}{expected}\n", "bumped\n".repeat(calls * lines_per_call));
    (b.program(vec![main, bump_def]), output)
}

#[test]
fn comments_never_change_what_a_program_does() {
    if !gxx::available() {
        return;
    }
    let mut comments = sample(comment_text(), 300);
    comments.extend(TRICKY.iter().map(|t| (*t).to_owned()));
    let (program, expected) = counting_program(&comments);
    let project = generate_checked(&program, &options("Comment Round Trip"));
    let executable = gxx::build(&project, CHARSET_FLAGS).unwrap();
    let out = gxx::run(&executable, b"");
    assert!(out.status.success());
    assert_eq!(out.stdout(), expected);
}

/// Decodes a C++ narrow string literal as the encoder writes it.
fn decode_cpp_literal(literal: &str) -> String {
    let inner = literal
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap();
    let mut out = String::new();
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        assert!(c != '"' && !c.is_control(), "raw {c:?} in {literal}");
        if c != '\\' {
            out.push(c);
            continue;
        }
        let escape = chars.next().unwrap();
        let mut hex = |n: usize| {
            let digits: String = (0..n).map(|_| chars.next().unwrap()).collect();
            char::from_u32(u32::from_str_radix(&digits, 16).unwrap()).unwrap()
        };
        match escape {
            '\\' | '"' | '\'' | '?' => out.push(escape),
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            'u' => out.push(hex(4)),
            'U' => out.push(hex(8)),
            '0'..='7' => {
                let mut value = escape.to_digit(8).unwrap();
                for _ in 0..2 {
                    value = value * 8 + chars.next().unwrap().to_digit(8).unwrap();
                }
                out.push(char::from_u32(value).unwrap());
            }
            other => panic!("unexpected escape \\{other} in {literal}"),
        }
    }
    out
}

/// Extracts the single string literal from a line like `    std::cout << "…" << '\n';`.
fn literal_in(line: &str) -> &str {
    let start = line.find('"').unwrap();
    let end = line.rfind('"').unwrap();
    &line[start..=end]
}

proptest! {
    #[test]
    fn emitted_string_literals_decode_to_their_text(text in literal_text()) {
        let b = Builder::new();
        let main = b.main(vec![b.print(vec![b.str(&text)])]);
        let project = generate(&b.program(vec![main]), &options("P"));
        let contents = &project.files[0].contents;
        let line = contents.lines().find(|l| l.trim_start().starts_with("std::cout")).unwrap();
        prop_assert_eq!(decode_cpp_literal(literal_in(line)), text);
    }

    #[test]
    fn comments_only_add_comment_lines(comments in prop::collection::vec(comment_text(), 1..6)) {
        let strip = |program: &Program| -> Vec<String> {
            let project = generate(program, &options("P"));
            crate::assert_style(&project);
            project.files[0]
                .contents
                .lines()
                .filter(|line| !line.trim_start().starts_with("//"))
                .map(str::to_owned)
                .collect()
        };
        let (with_comments, _) = counting_program(&comments);
        let mut without = with_comments.clone();
        strip_comments(&mut without);
        prop_assert_eq!(strip(&with_comments), strip(&without));
    }
}

/// Removes every comment from a program.
fn strip_comments(program: &mut Program) {
    fn strip(block: &mut b2c_ir::sast::Block) {
        for stmt in &mut block.stmts {
            stmt.comment = None;
            match &mut stmt.kind {
                StmtKind::If { branches, else_body } => {
                    for branch in branches {
                        strip(&mut branch.body);
                    }
                    if let Some(body) = else_body {
                        strip(body);
                    }
                }
                StmtKind::While { body, .. }
                | StmtKind::Repeat { body, .. }
                | StmtKind::ForRange { body, .. }
                | StmtKind::Forever { body } => strip(body),
                _ => {}
            }
        }
    }
    for module in &mut program.modules {
        for item in &mut module.items {
            item.comment = None;
            match &mut item.kind {
                b2c_ir::sast::ItemKind::Main(def) => strip(&mut def.body),
                b2c_ir::sast::ItemKind::Function(def) => strip(&mut def.body),
            }
        }
    }
}
