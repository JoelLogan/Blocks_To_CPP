//! Source maps: well-formedness, ranges that cover exactly the code of their
//! statement or expression, and lookups from compiler positions to blocks.

use b2c_codegen::generate;
use b2c_ir::diag::Part;
use b2c_ir::sast::{Block, Expr, ExprKind, ItemKind, Origin, Program, Stmt, StmtKind};
use b2c_ir::source_map::{GeneratedProject, MappedRange, Position};
use b2c_ir::types::Type;

use crate::builder::{Builder, commented};
use crate::{examples, options};

/// Checks the invariants every source map must satisfy: one map per file,
/// ranges sorted by start, non-empty, inside the file, and properly nested.
pub(crate) fn assert_well_formed(project: &GeneratedProject) {
    let map = &project.source_map;
    assert_eq!(map.version, 1);
    assert_eq!(map.files.len(), project.files.len());
    for (file, file_map) in project.files.iter().zip(&map.files) {
        assert_eq!(file.path, file_map.path);
        let lines: Vec<&str> = file.contents.lines().collect();
        let column_ok = |p: Position| {
            let line = lines
                .get(p.line as usize - 1)
                .unwrap_or_else(|| panic!("line {} is outside {}", p.line, file.path));
            p.column >= 1
                && p.column as usize <= line.len() + 1
                && line.is_char_boundary(p.column as usize - 1)
        };
        let mut open: Vec<&MappedRange> = Vec::new();
        for (index, range) in file_map.ranges.iter().enumerate() {
            assert!(range.start < range.end, "empty range {range:?}");
            assert!(
                column_ok(range.start) && column_ok(range.end),
                "range {range:?} is outside the text"
            );
            if index > 0 {
                assert!(
                    file_map.ranges[index - 1].start <= range.start,
                    "ranges must be sorted by start"
                );
            }
            while open.last().is_some_and(|top| top.end <= range.start) {
                open.pop();
            }
            if let Some(top) = open.last() {
                assert!(
                    range.end <= top.end,
                    "{range:?} overlaps {top:?} without being inside it"
                );
            }
            open.push(range);
        }
    }
}

/// The text a range covers.
fn text_of(contents: &str, range: &MappedRange) -> String {
    let offset = |p: Position| {
        let line_start: usize = contents
            .split_inclusive('\n')
            .take(p.line as usize - 1)
            .map(str::len)
            .sum();
        line_start + p.column as usize - 1
    };
    contents[offset(range.start)..offset(range.end)].to_owned()
}

/// The position of the first occurrence of `needle` in `contents`.
fn position_of(contents: &str, needle: &str) -> Position {
    let offset = contents
        .find(needle)
        .unwrap_or_else(|| panic!("{needle:?} not found"));
    let before = &contents[..offset];
    let line = before.matches('\n').count() + 1;
    let column = offset - before.rfind('\n').map_or(0, |i| i + 1) + 1;
    Position {
        line: u32::try_from(line).unwrap(),
        column: u32::try_from(column).unwrap(),
    }
}

/// Every statement and expression of a program, in order.
fn nodes(program: &Program) -> (Vec<&Stmt>, Vec<&Expr>) {
    fn block<'a>(b: &'a Block, stmts: &mut Vec<&'a Stmt>, exprs: &mut Vec<&'a Expr>) {
        for stmt in &b.stmts {
            stmts.push(stmt);
            let mut e = |x: &'a Expr| expr(x, exprs);
            match &stmt.kind {
                StmtKind::VarDecl(d) => d.init.iter().for_each(&mut e),
                StmtKind::Assign { value, .. } | StmtKind::CompoundAssign { value, .. } => e(value),
                StmtKind::If { branches, else_body } => {
                    for branch in branches {
                        expr(&branch.cond, exprs);
                        block(&branch.body, stmts, exprs);
                    }
                    if let Some(body) = else_body {
                        block(body, stmts, exprs);
                    }
                }
                StmtKind::While { cond, body, .. } | StmtKind::Repeat { count: cond, body } => {
                    expr(cond, exprs);
                    block(body, stmts, exprs);
                }
                StmtKind::ForRange {
                    from, to, step, body, ..
                } => {
                    expr(from, exprs);
                    expr(to, exprs);
                    if let Some(step) = step {
                        expr(step, exprs);
                    }
                    block(body, stmts, exprs);
                }
                StmtKind::Forever { body } => block(body, stmts, exprs),
                StmtKind::Return { value: x }
                | StmtKind::Exit { code: x, .. }
                | StmtKind::Ask { prompt: x, .. } => {
                    x.iter().for_each(e);
                }
                StmtKind::Eval { expr: x } => e(x),
                StmtKind::Print { items, .. } => items.iter().for_each(e),
                StmtKind::Break | StmtKind::Continue => {}
            }
        }
    }
    fn expr<'a>(x: &'a Expr, exprs: &mut Vec<&'a Expr>) {
        exprs.push(x);
        match &x.kind {
            ExprKind::Unary { operand, .. } | ExprKind::Convert { value: operand, .. } => {
                expr(operand, exprs);
            }
            ExprKind::Binary { lhs, rhs, .. } | ExprKind::RandomInt { low: lhs, high: rhs } => {
                expr(lhs, exprs);
                expr(rhs, exprs);
            }
            ExprKind::Conditional {
                cond,
                then_value,
                else_value,
            } => {
                for x in [cond, then_value, else_value] {
                    expr(x, exprs);
                }
            }
            ExprKind::Call { args: items, .. } | ExprKind::Join(items) => {
                for item in items {
                    expr(item, exprs);
                }
            }
            _ => {}
        }
    }
    let (mut stmts, mut exprs) = (Vec::new(), Vec::new());
    for item in program.modules.iter().flat_map(|m| &m.items) {
        match &item.kind {
            ItemKind::Main(def) => block(&def.body, &mut stmts, &mut exprs),
            ItemKind::Function(def) => block(&def.body, &mut stmts, &mut exprs),
        }
    }
    (stmts, exprs)
}

/// The ranges mapped to an origin.
fn ranges_of<'a>(project: &'a GeneratedProject, origin: &Origin) -> Vec<&'a MappedRange> {
    project.source_map.files[0]
        .ranges
        .iter()
        .filter(|r| r.block == origin.block && r.part == origin.part && r.module == origin.module)
        .collect()
}

#[test]
fn every_statement_range_covers_exactly_its_code() {
    let program = examples::guessing_game();
    let project = generate(&program, &options("Guessing Game"));
    let contents = &project.files[0].contents;
    let (stmts, _) = nodes(&program);
    let mut seen = Vec::new();
    for stmt in stmts {
        let ranges = ranges_of(&project, &stmt.origin);
        assert_eq!(ranges.len(), 1, "{:?}", stmt.kind);
        let text = text_of(contents, ranges[0]);
        assert!(text.ends_with(';') || text.ends_with('}'), "{text}");
        assert_eq!(text.matches('{').count(), text.matches('}').count(), "{text}");
        assert_eq!(text.matches('(').count(), text.matches(')').count(), "{text}");
        seen.push(text.lines().next().unwrap().to_owned());
    }
    assert_eq!(
        seen,
        [
            "int secret = b2c::random_int(1, 100);",
            "int guess = 0;",
            "std::cout << \"Guess a number from 1 to 100!\" << '\\n';",
            "while (guess != secret) {",
            "guess = b2c::ask<int>(\"Your guess: \");",
            "if (guess < secret) {",
            "std::cout << \"Too low!\" << '\\n';",
            "std::cout << \"Too high!\" << '\\n';",
            "std::cout << \"Correct!\" << '\\n';",
        ]
    );
}

#[test]
fn every_expression_range_covers_exactly_its_code() {
    let program = examples::guessing_game();
    let project = generate(&program, &options("Guessing Game"));
    let contents = &project.files[0].contents;
    let (_, exprs) = nodes(&program);
    let texts: Vec<String> = exprs
        .iter()
        .map(|e| {
            let ranges = ranges_of(&project, &e.origin);
            assert_eq!(ranges.len(), 1, "{:?}", e.kind);
            text_of(contents, ranges[0])
        })
        .collect();
    assert_eq!(
        texts,
        [
            "b2c::random_int(1, 100)",
            "1",
            "100",
            "0",
            "\"Guess a number from 1 to 100!\"",
            // `repeat until guess == secret` is printed as `guess != secret`.
            "guess != secret",
            "guess",
            "secret",
            "\"Your guess: \"",
            "guess < secret",
            "guess",
            "secret",
            "\"Too low!\"",
            "guess > secret",
            "guess",
            "secret",
            "\"Too high!\"",
            "\"Correct!\"",
        ]
    );
}

#[test]
fn every_node_of_the_functions_example_is_mapped() {
    let program = examples::functions();
    let project = generate(&program, &options("Functions"));
    let (stmts, exprs) = nodes(&program);
    for origin in stmts
        .iter()
        .map(|s| &s.origin)
        .chain(exprs.iter().map(|e| &e.origin))
    {
        assert!(!ranges_of(&project, origin).is_empty(), "{origin:?} has no range");
    }
    // Each function maps its forward declaration and its definition, and each
    // parameter appears in both.
    let contents = &project.files[0].contents;
    for item in &program.modules[0].items {
        let ranges = ranges_of(&project, &item.origin);
        let texts: Vec<String> = ranges.iter().map(|r| text_of(contents, r)).collect();
        if let ItemKind::Function(def) = &item.kind {
            assert_eq!(texts.len(), 2, "{texts:?}");
            assert!(texts[0].ends_with(");"), "{}", texts[0]);
            assert!(texts[1].ends_with('}'), "{}", texts[1]);
            for param in &def.params {
                assert_eq!(ranges_of(&project, &param.origin).len(), 2);
            }
        } else {
            assert_eq!(texts.len(), 1);
            assert!(
                texts[0].starts_with("int main() {") && texts[0].ends_with("    return 0;\n}"),
                "{}",
                texts[0]
            );
        }
    }
    // `greet` is the second item; its first parameter is read-only text.
    let ItemKind::Function(greet) = &program.modules[0].items[1].kind else {
        panic!("greet")
    };
    let param = &greet.params[0].origin;
    assert_eq!(
        param.part,
        Part::Field {
            name: String::from("PARAM0")
        }
    );
    let texts: Vec<String> = ranges_of(&project, param)
        .iter()
        .map(|r| text_of(contents, r))
        .collect();
    assert_eq!(texts, ["const std::string& name", "const std::string& name"]);
}

#[test]
fn lookup_finds_the_innermost_block() {
    let program = examples::guessing_game();
    let project = generate(&program, &options("Guessing Game"));
    let contents = &project.files[0].contents;
    let map = &project.source_map;
    let (stmts, exprs) = nodes(&program);
    let block_at = |needle: &str| {
        map.lookup("main.cpp", position_of(contents, needle))
            .map(|r| r.block.clone())
    };

    let too_low_literal = exprs
        .iter()
        .find(|e| matches!(&e.kind, ExprKind::Str(s) if s.value() == "Too low!"))
        .unwrap();
    assert_eq!(
        block_at("\"Too low!\""),
        Some(too_low_literal.origin.block.clone())
    );
    let too_low_print = stmts
        .iter()
        .find(
            |s| matches!(&s.kind, StmtKind::Print { items, .. } if items[0].origin == too_low_literal.origin),
        )
        .unwrap();
    assert_eq!(
        block_at("std::cout << \"Too low!\""),
        Some(too_low_print.origin.block.clone())
    );
    let ask = stmts
        .iter()
        .find(|s| matches!(s.kind, StmtKind::Ask { .. }))
        .unwrap();
    assert_eq!(block_at("guess = b2c::ask"), Some(ask.origin.block.clone()));
    // A GCC error at `!=` (inside the condition) maps to the condition block.
    let cond = exprs
        .iter()
        .find(|e| {
            matches!(
                &e.kind,
                ExprKind::Binary {
                    op: b2c_ir::sast::BinaryOp::Eq,
                    ..
                }
            )
        })
        .unwrap();
    assert_eq!(block_at("!= secret"), Some(cond.origin.block.clone()));
    // The support helpers and the includes are not mapped.
    assert_eq!(block_at("inline int random_int"), None);
    assert_eq!(block_at("#include <iostream>"), None);
    // Line-only lookups (e.g. from the linker) find something on mapped lines.
    let line = position_of(contents, "\"Too high!\"").line;
    assert!(map.lookup_line("main.cpp", line).is_some());
}

#[test]
fn comment_lines_belong_to_their_statement() {
    let b = Builder::new();
    let x = b.var("x", Type::Int);
    let declare = commented(b.declare(&x, Some(b.int("1"))), "first\nsecond");
    let origin = declare.origin.clone();
    let main = b.main(vec![declare, b.print(vec![b.get(&x)])]);
    let project = generate(&b.program(vec![main]), &options("P"));
    let contents = &project.files[0].contents;
    let range = ranges_of(&project, &origin)[0];
    assert_eq!(
        text_of(contents, range),
        "// first\n    // second\n    int x = 1;"
    );
    assert_eq!(
        project
            .source_map
            .lookup("main.cpp", position_of(contents, "second"))
            .unwrap()
            .block,
        origin.block
    );
}

#[test]
fn columns_count_utf8_bytes() {
    let b = Builder::new();
    let x = b.var("x", Type::Int);
    let item = b.get(&x);
    let origin = item.origin.clone();
    let main = b.main(vec![
        b.declare(&x, Some(b.int("1"))),
        b.print(vec![b.str("é✓😀"), item]),
    ]);
    let project = generate(&b.program(vec![main]), &options("P"));
    let range = ranges_of(&project, &origin)[0];
    // `    std::cout << "é✓😀" << x`: 4 + 13 + 11 bytes (é 2, ✓ 3, 😀 4, quotes 2) + 4.
    assert_eq!(range.start.column, 4 + 13 + 11 + 4 + 1);
    assert_eq!(range.end.column, range.start.column + 1);
    assert_eq!(text_of(&project.files[0].contents, range), "x");
}

#[test]
fn parts_are_preserved() {
    let b = Builder::new();
    let x = b.var("x", Type::Int);
    let mut value = b.int("41");
    value.origin = b.input_origin("VALUE");
    let origin = value.origin.clone();
    let main = b.main(vec![b.declare(&x, Some(value)), b.print(vec![b.get(&x)])]);
    let project = generate(&b.program(vec![main]), &options("P"));
    let range = ranges_of(&project, &origin)[0];
    assert_eq!(
        range.part,
        Part::Input {
            name: String::from("VALUE")
        }
    );
    assert_eq!(text_of(&project.files[0].contents, range), "41");
}
