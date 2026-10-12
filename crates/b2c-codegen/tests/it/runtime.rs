//! Builds generated programs with g++ and runs them with stdin fixtures: the
//! support helpers (`ask<T>`, `ask_line`, `random_int`, `to_text`) and the
//! semantics of the desugared statements.

use b2c_ir::sast::{
    AskMode, BinaryOp, CompoundOp, OutputStream, PassMode, PrintSeparator, Program, RangeDirection, StmtKind,
};
use b2c_ir::types::Type;

use crate::builder::{Builder, block};
use crate::gxx::{self, Run};
use crate::{examples, generate_checked, options};

/// Generates, builds and runs a program. `None` when g++ is not available.
fn run(program: &Program, stdin: &str) -> Option<Run> {
    let project = generate_checked(program, &options("Runtime Tests"));
    let executable = gxx::build(&project, &[])?;
    Some(gxx::run(&executable, stdin.as_bytes()))
}

/// A program that asks for one value of type `ty` and prints it.
fn ask_and_print(ty: Type, prompt: &str) -> Program {
    let b = Builder::new();
    let answer = b.var("answer", ty);
    let main = b.main(vec![
        b.declare(&answer, None),
        b.ask(Some(b.str(prompt)), &answer, AskMode::KeepAsking),
        b.print(vec![b.str("["), b.get(&answer), b.str("]")]),
    ]);
    b.program(vec![main])
}

#[test]
fn ask_int_asks_again_after_invalid_input() {
    let Some(out) = run(&ask_and_print(Type::Int, "Age? "), "abc\n\n42 and more\n42\n") else {
        return;
    };
    assert!(out.status.success(), "{}", out.stderr());
    assert_eq!(
        out.stdout(),
        "Age? Please enter a whole number.\n".repeat(3) + "Age? [42]\n",
        "a word, an empty line and a number with more after it are each refused"
    );
    assert_eq!(out.stderr(), "");
}

#[test]
fn ask_accepts_only_a_whole_line_holding_one_value() {
    const AGAIN: &str = "N? Please enter a whole number.\n";
    let program = ask_and_print(Type::Int, "N? ");
    for (stdin, expected) in [
        ("3.5\n4\n", format!("{AGAIN}N? [4]\n")),
        ("12abc\n7\n", format!("{AGAIN}N? [7]\n")),
        ("0x10\n1 2\n+8\n", format!("{AGAIN}{AGAIN}N? [8]\n")),
        ("  7  \n", String::from("N? [7]\n")),
        ("\t-7\r\n", String::from("N? [-7]\n")),
        ("7", String::from("N? [7]\n")),
    ] {
        let Some(out) = run(&program, stdin) else { return };
        assert!(out.status.success(), "stdin {stdin:?}: {}", out.stderr());
        assert_eq!(out.stdout(), expected, "stdin {stdin:?}");
    }
    let program = ask_and_print(Type::Double, "X? ");
    let Some(out) = run(&program, "1e400\n2.5x\n2.5\n") else {
        return;
    };
    assert_eq!(
        out.stdout(),
        "X? Please enter a number.\n".repeat(2) + "X? [2.5]\n"
    );
}

#[test]
fn ask_int_rejects_numbers_that_do_not_fit() {
    let Some(out) = run(&ask_and_print(Type::Int, "N? "), "99999999999\n-7\n") else {
        return;
    };
    assert_eq!(out.stdout(), "N? Please enter a whole number.\nN? [-7]\n");
}

#[test]
fn ask_stops_at_end_of_input() {
    let program = ask_and_print(Type::Int, "Age? ");
    for (stdin, expected_stdout) in [
        ("", "Age? "),
        ("abc", "Age? Please enter a whole number.\nAge? "),
        (
            "x\ny\n",
            "Age? Please enter a whole number.\nAge? Please enter a whole number.\nAge? ",
        ),
    ] {
        let Some(out) = run(&program, stdin) else { return };
        assert_eq!(out.status.code(), Some(1), "stdin {stdin:?}");
        assert_eq!(out.stdout(), expected_stdout, "stdin {stdin:?}");
        assert_eq!(out.stderr(), "Input ended\n", "stdin {stdin:?}");
    }
}

#[test]
fn ask_line_reads_a_whole_line() {
    let program = ask_and_print(Type::String, "Name? ");
    let Some(out) = run(&program, "   Ada Lovelace\r\nignored\n") else {
        return;
    };
    assert!(out.status.success(), "{}", out.stderr());
    assert_eq!(out.stdout(), "Name? [Ada Lovelace]\n");
    let Some(out) = run(&program, "Grace") else { return };
    assert_eq!(
        out.stdout(),
        "Name? [Grace]\n",
        "a last line without a line break"
    );
    let Some(out) = run(&program, "\n\n") else { return };
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(out.stdout(), "Name? Name? Name? ");
    assert_eq!(out.stderr(), "Input ended\n");
}

#[test]
fn ask_line_asks_again_after_an_empty_line() {
    let program = ask_and_print(Type::String, "Name? ");
    let Some(out) = run(&program, "\n\nBob\n") else {
        return;
    };
    assert!(out.status.success(), "{}", out.stderr());
    assert_eq!(out.stdout(), "Name? Name? Name? [Bob]\n");
    let Some(out) = run(&program, "  \t\r\nBob\n") else {
        return;
    };
    assert_eq!(out.stdout(), "Name? Name? [Bob]\n", "a line of spaces is empty");
}

#[test]
fn ask_reads_bool_double_and_char() {
    let b = Builder::new();
    let likes = b.var("likes", Type::Bool);
    let height = b.var("height", Type::Double);
    let grade = b.var("grade", Type::Char);
    let main = b.main(vec![
        b.declare(&likes, None),
        b.declare(&height, None),
        b.declare(&grade, None),
        b.ask(Some(b.str("Like? ")), &likes, AskMode::KeepAsking),
        b.ask(Some(b.str("Height? ")), &height, AskMode::KeepAsking),
        b.ask(Some(b.chr(">")), &grade, AskMode::KeepAsking),
        b.print_with(
            vec![b.get(&likes), b.get(&height), b.get(&grade)],
            PrintSeparator::Space,
            true,
            OutputStream::Out,
        ),
    ]);
    let program = b.program(vec![main]);
    let cases = [
        (
            "maybe\nYES\ntall\n3.5\n  x  \n",
            "Like? Please enter true or false.\nLike? Height? Please enter a number.\nHeight? >true 3.5 x\n",
        ),
        ("no\n2.25\nq\n", "Like? Height? >false 2.25 q\n"),
        ("1\n1e2\n7\n", "Like? Height? >true 100 7\n"),
        ("0\n0\n#\n", "Like? Height? >false 0 #\n"),
        ("True\n-1.5\nA\n", "Like? Height? >true -1.5 A\n"),
        (
            "yes please\nno\n1.5.\n1.5\nAb\n\nb\n",
            "Like? Please enter true or false.\nLike? Height? Please enter a number.\nHeight? \
             >Please enter one character.\n>Please enter one character.\n>false 1.5 b\n",
        ),
    ];
    for (stdin, expected) in cases {
        let Some(out) = run(&program, stdin) else { return };
        assert!(out.status.success(), "{}", out.stderr());
        assert_eq!(out.stdout(), expected, "stdin {stdin:?}");
    }
}

#[test]
fn simple_ask_uses_plain_streams() {
    let b = Builder::new();
    let n = b.var("n", Type::Int);
    let name = b.var("name", Type::String);
    let main = b.main(vec![
        b.declare(&n, None),
        b.declare(&name, None),
        b.ask(None, &n, AskMode::Simple),
        b.ask(Some(b.str("Name? ")), &name, AskMode::Simple),
        b.print_with(
            vec![b.get(&n), b.get(&name)],
            PrintSeparator::Comma,
            true,
            OutputStream::Out,
        ),
    ]);
    let Some(out) = run(&b.program(vec![main]), "7\nBob Smith\n") else {
        return;
    };
    assert_eq!(out.stdout(), "Name? 7, Bob Smith\n");
}

/// A program that asks for `first` in `first_mode`, then for `second` with
/// keep asking, and prints both.
fn ask_twice(first: Type, first_mode: AskMode, second: Type) -> Program {
    let b = Builder::new();
    let a = b.var("a", first);
    let z = b.var("z", second);
    let main = b.main(vec![
        b.declare(&a, None),
        b.declare(&z, None),
        b.ask(Some(b.str("A? ")), &a, first_mode),
        b.ask(Some(b.str("Z? ")), &z, AskMode::KeepAsking),
        b.print_with(
            vec![b.get(&a), b.get(&z)],
            PrintSeparator::Comma,
            true,
            OutputStream::Out,
        ),
    ]);
    b.program(vec![main])
}

#[test]
fn keep_asking_after_a_simple_ask_reads_the_next_line() {
    for (first, second, stdin, expected) in [
        (Type::Int, Type::String, "42\nAda\n", "A? Z? 42, Ada\n"),
        (Type::Int, Type::Int, "42\n9\n", "A? Z? 42, 9\n"),
        (Type::Int, Type::Int, "42 and more\n9\n", "A? Z? 42, 9\n"),
        (Type::Char, Type::Bool, "y\nyes\n", "A? Z? y, true\n"),
        (Type::Double, Type::Char, "2.5\nq\n", "A? Z? 2.5, q\n"),
        (Type::String, Type::Int, "Ada\n9\n", "A? Z? Ada, 9\n"),
    ] {
        let program = ask_twice(first.clone(), AskMode::Simple, second.clone());
        let Some(out) = run(&program, stdin) else { return };
        assert!(
            out.status.success(),
            "{first:?} then {second:?}: {}",
            out.stderr()
        );
        assert_eq!(
            out.stdout(),
            expected,
            "{first:?} then {second:?}, stdin {stdin:?}"
        );
        assert_eq!(out.stderr(), "");
    }
}

#[test]
fn keep_asking_after_a_failed_simple_ask_drops_its_line() {
    for (second, stdin, expected) in [
        (Type::Int, "abc\n9\n", "A? Z? 0, 9\n"),
        (Type::String, "abc\nAda\n", "A? Z? 0, Ada\n"),
    ] {
        let program = ask_twice(Type::Int, AskMode::Simple, second.clone());
        let Some(out) = run(&program, stdin) else { return };
        assert!(out.status.success(), "{second:?}: {}", out.stderr());
        assert_eq!(out.stdout(), expected, "{second:?}, stdin {stdin:?}");
    }
    let program = ask_twice(Type::Int, AskMode::Simple, Type::Int);
    let Some(out) = run(&program, "abc") else { return };
    assert_eq!(
        out.status.code(),
        Some(1),
        "the input ended after the failed read"
    );
    assert_eq!(out.stdout(), "A? Z? ");
    assert_eq!(out.stderr(), "Input ended\n");
}

#[test]
fn random_int_stays_within_its_bounds() {
    let b = Builder::new();
    let print_random = |low: &str, high: &str| b.print(vec![b.random(b.int(low), b.int(high))]);
    let main = b.main(vec![
        b.repeat(b.int("2000"), vec![print_random("3", "7")]),
        b.repeat(b.int("50"), vec![print_random("5", "5")]),
        b.repeat(b.int("500"), vec![print_random("9", "2")]),
        b.repeat(
            b.int("100"),
            vec![b.print(vec![
                b.random(b.un(b2c_ir::sast::UnaryOp::Neg, b.int("2")), b.int("0")),
            ])],
        ),
    ]);
    let Some(out) = run(&b.program(vec![main]), "") else {
        return;
    };
    let values: Vec<i32> = out.stdout().lines().map(|l| l.parse().unwrap()).collect();
    assert_eq!(values.len(), 2650);
    let (first, rest) = values.split_at(2000);
    let (second, rest) = rest.split_at(50);
    let (third, fourth) = rest.split_at(500);
    assert!(first.iter().all(|v| (3..=7).contains(v)));
    assert!(
        first.contains(&3) && first.contains(&7),
        "both bounds are included"
    );
    assert!(second.iter().all(|v| *v == 5));
    assert!(
        third.iter().all(|v| (2..=9).contains(v)),
        "swapped bounds still work"
    );
    assert!(fourth.iter().all(|v| (-2..=0).contains(v)));
}

#[test]
fn guessing_game_plays_to_the_end() {
    let guesses: Vec<String> = (1..=100).map(|guess| guess.to_string()).collect();
    let stdin = format!("abc\n{}\n", guesses.join("\n"));
    let Some(out) = run(&examples::guessing_game(), &stdin) else {
        return;
    };
    assert!(out.status.success(), "{}", out.stderr());
    let stdout = out.stdout();
    assert!(stdout.starts_with("Guess a number from 1 to 100!\nYour guess: Please enter a whole number.\n"));
    assert_eq!(stdout.matches("Correct!").count(), 1);
    assert!(
        !stdout.contains("Too high!"),
        "guesses go up from 1, so none is too high"
    );
    assert!(stdout.ends_with("Correct!\n"));
}

#[test]
fn functions_example_runs() {
    let Some(out) = run(&examples::functions(), "") else {
        return;
    };
    assert!(out.status.success(), "{}", out.stderr());
    assert_eq!(out.stdout(), "Hello, Ada!\nHello, Ada!\nGrace is happy\n130\n");
}

#[test]
#[allow(clippy::too_many_lines)] // a literal test program, clearest in one piece
fn loops_run_the_right_number_of_times() {
    let b = Builder::new();
    let n = b.var("n", Type::Int);
    let limit = b.var("limit", Type::Int);
    let k = b.var("k", Type::Int);
    let show = |e| b.print_with(vec![e], PrintSeparator::None, false, OutputStream::Out);
    let newline = || b.print(Vec::new());
    let (i1, i2, i3, i4, i5) = (
        b.loop_var("i"),
        b.loop_var("i"),
        b.loop_var("i"),
        b.loop_var("i"),
        b.loop_var("i"),
    );
    let main = b.main(vec![
        b.declare(&n, Some(b.int("3"))),
        b.repeat(
            b.get(&n),
            vec![
                show(b.str("x")),
                b.stmt(StmtKind::CompoundAssign {
                    target: n.clone(),
                    op: CompoundOp::Add,
                    value: b.int("1"),
                }),
            ],
        ),
        newline(),
        b.for_range(
            &i1,
            b.int("1"),
            b.int("9"),
            Some(b.int("2")),
            RangeDirection::UpInclusive,
            vec![show(b.get(&i1))],
        ),
        newline(),
        b.for_range(
            &i2,
            b.int("10"),
            b.int("1"),
            Some(b.int("3")),
            RangeDirection::DownInclusive,
            vec![show(b.get(&i2))],
        ),
        newline(),
        b.for_range(
            &i3,
            b.int("0"),
            b.int("3"),
            None,
            RangeDirection::UpExclusive,
            vec![show(b.get(&i3))],
        ),
        newline(),
        b.declare(&limit, Some(b.int("4"))),
        b.for_range(
            &i4,
            b.int("0"),
            b.get(&limit),
            None,
            RangeDirection::UpInclusive,
            vec![
                show(b.get(&i4)),
                b.stmt(StmtKind::CompoundAssign {
                    target: limit.clone(),
                    op: CompoundOp::Sub,
                    value: b.int("1"),
                }),
            ],
        ),
        newline(),
        b.for_range(
            &i5,
            b.int("3"),
            b.int("1"),
            None,
            RangeDirection::DownInclusive,
            vec![show(b.get(&i5))],
        ),
        newline(),
        b.declare(&k, Some(b.int("0"))),
        b.stmt(StmtKind::While {
            cond: b.bin(BinaryOp::Ge, b.get(&k), b.int("3")),
            until: true,
            body: block(vec![
                show(b.get(&k)),
                b.stmt(StmtKind::CompoundAssign {
                    target: k.clone(),
                    op: CompoundOp::Add,
                    value: b.int("1"),
                }),
            ]),
        }),
        newline(),
        b.stmt(StmtKind::Forever {
            body: block(vec![
                b.stmt(StmtKind::CompoundAssign {
                    target: k.clone(),
                    op: CompoundOp::Add,
                    value: b.int("1"),
                }),
                b.if_else(
                    vec![(
                        b.bin(BinaryOp::Eq, b.get(&k), b.int("5")),
                        vec![b.stmt(StmtKind::Continue)],
                    )],
                    None,
                ),
                b.if_else(
                    vec![(
                        b.bin(BinaryOp::Gt, b.get(&k), b.int("7")),
                        vec![b.stmt(StmtKind::Break)],
                    )],
                    None,
                ),
                show(b.get(&k)),
            ]),
        }),
        newline(),
    ]);
    let Some(out) = run(&b.program(vec![main]), "") else {
        return;
    };
    assert_eq!(out.stdout(), "xxx\n13579\n10741\n012\n01234\n321\n012\n467\n");
}

#[test]
fn exit_codes() {
    let b = Builder::new();
    let stop = b.function("stop", Type::Void, &[("code", Type::Int, PassMode::Copy)]);
    let code = stop.params[0].clone();
    let stop_def = b.define(
        &stop,
        vec![
            b.print_text("stopping"),
            b.stmt(StmtKind::Exit {
                code: Some(b.get(&code)),
                in_main: false,
            }),
        ],
    );
    let main = b.main(vec![
        b.eval(b.call(&stop, vec![b.int("3")])),
        b.print_text("not reached"),
    ]);
    let Some(out) = run(&b.program(vec![main, stop_def]), "") else {
        return;
    };
    assert_eq!(out.status.code(), Some(3));
    assert_eq!(out.stdout(), "stopping\n", "std::exit flushes the output");

    let b = Builder::new();
    let main = b.main(vec![
        b.if_else(
            vec![(
                b.boolean(true),
                vec![b.stmt(StmtKind::Exit {
                    code: Some(b.int("4")),
                    in_main: true,
                })],
            )],
            None,
        ),
        b.print_text("not reached"),
    ]);
    let Some(out) = run(&b.program(vec![main]), "") else {
        return;
    };
    assert_eq!(out.status.code(), Some(4));
    assert_eq!(out.stdout(), "");
}

#[test]
fn printing_and_joining() {
    let b = Builder::new();
    let ok = b.var("ok", Type::Bool);
    let main = b.main(vec![
        b.declare(&ok, Some(b.bin(BinaryOp::Lt, b.int("1"), b.int("2")))),
        b.print_with(
            vec![
                b.boolean(true),
                b.get(&ok),
                b.bin(BinaryOp::Gt, b.int("1"), b.int("2")),
                b.chr("c"),
                b.float("0.5"),
            ],
            PrintSeparator::Space,
            true,
            OutputStream::Out,
        ),
        b.print_with(
            vec![b.int("1"), b.int("2"), b.int("3")],
            PrintSeparator::Comma,
            true,
            OutputStream::Out,
        ),
        b.print_with(
            vec![b.str("to stderr")],
            PrintSeparator::None,
            true,
            OutputStream::Err,
        ),
        b.print(vec![b.join(vec![
            b.str("Score: "),
            b.int("42"),
            b.str(" "),
            b.chr("A"),
            b.str(" "),
            b.get(&ok),
            b.str(" "),
            b.float("0.5"),
        ])]),
        b.print(vec![b.bin(BinaryOp::Add, b.str("con"), b.str("cat"))]),
        b.print(vec![b.bin(BinaryOp::Lt, b.str("apple"), b.str("banana"))]),
        b.print(vec![b.join(vec![b.chr("x"), b.boolean(false)])]),
    ]);
    let Some(out) = run(&b.program(vec![main]), "") else {
        return;
    };
    assert_eq!(
        out.stdout(),
        "true true false c 0.5\n1, 2, 3\nScore: 42 A true 0.5\nconcat\ntrue\nxfalse\n"
    );
    assert_eq!(out.stderr(), "to stderr\n");
}

#[test]
fn join_writes_decimal_numbers_as_print_does() {
    let b = Builder::new();
    let x = b.var("x", Type::Double);
    let name = b.var("name", Type::String);
    let values = ["2.5", "0.1", "1e20", "1.0", "0.000001", "123456789.0"];
    let mut body = vec![b.declare(&x, Some(b.float("0.0")))];
    for value in values {
        body.push(b.set(&x, b.float(value)));
        body.push(b.print(vec![b.get(&x)]));
        // As the first part of a join, and after text.
        body.push(b.print(vec![b.join(vec![b.get(&x)])]));
        body.push(b.print(vec![b.join(vec![b.str("="), b.get(&x)])]));
    }
    // A decimal prompt is written the same way.
    body.push(b.declare(&name, None));
    body.push(b.ask(Some(b.float("2.5")), &name, AskMode::KeepAsking));
    let main = b.main(body);
    let Some(out) = run(&b.program(vec![main]), "Ada\n") else {
        return;
    };
    assert!(out.status.success(), "{}", out.stderr());
    let stdout = out.stdout();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), values.len() * 3 + 1, "{lines:?}");
    for (index, chunk) in lines.chunks(3).take(values.len()).enumerate() {
        let printed = chunk[0];
        assert_eq!(chunk[1], printed, "join of {}", values[index]);
        assert_eq!(chunk[2], format!("={printed}"), "join of {}", values[index]);
    }
    assert_eq!(&lines[..3], ["2.5", "2.5", "=2.5"]);
    assert_eq!(lines[6], "1e+20");
    assert_eq!(
        lines.last(),
        Some(&"2.5"),
        "the prompt, with no line break after it"
    );
}
