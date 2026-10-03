//! Example programs shared by several test modules.

use b2c_ir::sast::{AskMode, BinaryOp, PassMode, Program, StmtKind};
use b2c_ir::types::Type;

use crate::builder::{Builder, block, commented, commented_item};

/// The guessing game of spec §3.13.1.
pub(crate) fn guessing_game() -> Program {
    let b = Builder::new();
    let secret = b.var("secret", Type::Int);
    let guess = b.var("guess", Type::Int);
    let body = vec![
        b.declare(&secret, Some(b.random(b.int("1"), b.int("100")))),
        b.declare(&guess, Some(b.int("0"))),
        b.print_text("Guess a number from 1 to 100!"),
        b.stmt(StmtKind::While {
            cond: b.bin(BinaryOp::Eq, b.get(&guess), b.get(&secret)),
            until: true,
            body: block(vec![
                b.ask(Some(b.str("Your guess: ")), &guess, AskMode::KeepAsking),
                b.if_else(
                    vec![
                        (
                            b.bin(BinaryOp::Lt, b.get(&guess), b.get(&secret)),
                            vec![b.print_text("Too low!")],
                        ),
                        (
                            b.bin(BinaryOp::Gt, b.get(&guess), b.get(&secret)),
                            vec![b.print_text("Too high!")],
                        ),
                    ],
                    Some(vec![b.print_text("Correct!")]),
                ),
            ]),
        }),
    ];
    let main = b.main(body);
    b.program(vec![main])
}

/// Functions with every parameter mode, return values and recursion. The
/// items are deliberately not in name order.
#[allow(clippy::too_many_lines)] // a literal test program, clearest in one piece
pub(crate) fn functions() -> Program {
    let b = Builder::new();
    let factorial = b.function("factorial", Type::Int, &[("n", Type::Int, PassMode::Copy)]);
    let greet = b.function(
        "greet",
        Type::Void,
        &[
            ("name", Type::String, PassMode::ReadOnly),
            ("times", Type::Int, PassMode::Copy),
        ],
    );
    let add_bonus = b.function(
        "add_bonus",
        Type::Void,
        &[
            ("score", Type::Int, PassMode::Editable),
            ("amount", Type::Int, PassMode::Copy),
        ],
    );
    let is_even = b.function("is_even", Type::Bool, &[("value", Type::Int, PassMode::ReadOnly)]);
    let describe = b.function(
        "describe",
        Type::String,
        &[
            ("who", Type::String, PassMode::ReadOnly),
            ("happy", Type::Bool, PassMode::Copy),
        ],
    );
    let [n] = factorial.params.as_slice() else {
        unreachable!()
    };
    let [name, times] = greet.params.as_slice() else {
        unreachable!()
    };
    let [score_param, amount] = add_bonus.params.as_slice() else {
        unreachable!()
    };
    let [value] = is_even.params.as_slice() else {
        unreachable!()
    };
    let [who, happy] = describe.params.as_slice() else {
        unreachable!()
    };

    let factorial_def = b.define(
        &factorial,
        vec![
            b.if_else(
                vec![(
                    b.bin(BinaryOp::Le, b.get(n), b.int("1")),
                    vec![b.ret(Some(b.int("1")))],
                )],
                None,
            ),
            b.ret(Some(b.bin(
                BinaryOp::Mul,
                b.get(n),
                b.call(&factorial, vec![b.bin(BinaryOp::Sub, b.get(n), b.int("1"))]),
            ))),
        ],
    );
    let factorial_def = commented_item(factorial_def, "Multiplies all whole numbers from 1 to n.");
    let greet_def = b.define(
        &greet,
        vec![b.repeat(
            b.get(times),
            vec![b.print(vec![b.str("Hello, "), b.get(name), b.str("!")])],
        )],
    );
    let add_bonus_def = b.define(
        &add_bonus,
        vec![b.stmt(StmtKind::CompoundAssign {
            target: score_param.clone(),
            op: b2c_ir::sast::CompoundOp::Add,
            value: b.get(amount),
        })],
    );
    let is_even_def = b.define(
        &is_even,
        vec![b.ret(Some(b.bin(
            BinaryOp::Eq,
            b.bin(BinaryOp::Mod, b.get(value), b.int("2")),
            b.int("0"),
        )))],
    );
    let describe_def = b.define(
        &describe,
        vec![b.ret(Some(b.join(vec![
            b.get(who),
            b.str(" is "),
            b.cond(b.get(happy), b.str("happy"), b.str("sad")),
        ])))],
    );

    let score = b.var("score", Type::Int);
    let main = b.main(vec![
        b.eval(b.call(&greet, vec![b.str("Ada"), b.int("2")])),
        commented(
            b.declare(&score, Some(b.call(&factorial, vec![b.int("5")]))),
            "5! = 120",
        ),
        b.eval(b.call(&add_bonus, vec![b.get(&score), b.int("10")])),
        b.if_else(
            vec![(
                b.call(&is_even, vec![b.get(&score)]),
                vec![b.print(vec![b.call(&describe, vec![b.str("Grace"), b.boolean(true)])])],
            )],
            None,
        ),
        b.print(vec![b.get(&score)]),
    ]);
    b.program(vec![
        main,
        greet_def,
        describe_def,
        factorial_def,
        is_even_def,
        add_bonus_def,
    ])
}
