//! Diagnostic codes reported by the analyser (spec §6.12). Every code is
//! documented in `docs/reference/diagnostics/analyser.md`; a test below keeps
//! the two in sync.

// --- Names and scopes (E02xx) ---------------------------------------------

/// A reference to a symbol that is declared nowhere.
pub(crate) const NOT_DECLARED: &str = "B2C-E0201";
/// A variable used before the block that creates it.
pub(crate) const USED_BEFORE_DECLARATION: &str = "B2C-E0202";
/// A symbol used outside the part of the program where it exists.
pub(crate) const OUT_OF_SCOPE: &str = "B2C-E0203";
/// A symbol declared in a disabled block or a block that is not attached.
pub(crate) const UNAVAILABLE: &str = "B2C-E0204";
/// A reference that C++ would resolve to another symbol with the same name.
pub(crate) const NAME_HIDDEN: &str = "B2C-E0205";
/// A function defined in another module.
pub(crate) const OTHER_MODULE: &str = "B2C-E0206";
/// A function used as a value, a variable called, or a function changed.
pub(crate) const WRONG_KIND: &str = "B2C-E0207";
/// Two variables or parameters with the same name in one scope.
pub(crate) const DUPLICATE_NAME: &str = "B2C-E0210";
/// Two functions with the same name in one module.
pub(crate) const DUPLICATE_FUNCTION: &str = "B2C-E0211";
/// Two declarations with the same symbol ID (a damaged file).
pub(crate) const DUPLICATE_SYMBOL_ID: &str = "B2C-E0212";
/// A name that is not a valid C++ identifier for user code.
pub(crate) const INVALID_NAME: &str = "B2C-E0220";

// --- Types and conversions (E03xx) -----------------------------------------

/// A value of the wrong type that cannot be converted.
pub(crate) const CONVERSION: &str = "B2C-E0301";
/// An operator used with operands it does not accept.
pub(crate) const BAD_OPERANDS: &str = "B2C-E0302";
/// `+` used on text (the join block is needed).
pub(crate) const TEXT_PLUS: &str = "B2C-E0303";
/// `%` (mod) used with decimal numbers.
pub(crate) const MOD_DECIMAL: &str = "B2C-E0304";
/// A function that gives back nothing used as a value.
pub(crate) const NO_VALUE: &str = "B2C-E0305";
/// A call with the wrong number of arguments.
pub(crate) const ARGUMENT_COUNT: &str = "B2C-E0306";
/// An editable parameter given something other than a matching variable.
pub(crate) const EDITABLE_ARGUMENT: &str = "B2C-E0307";
/// A change to a constant, a loop counter or a read-only parameter.
pub(crate) const NOT_ASSIGNABLE: &str = "B2C-E0308";
/// An `auto` variable without a starting value.
pub(crate) const AUTO_WITHOUT_VALUE: &str = "B2C-E0309";
/// Text that is not a valid number.
pub(crate) const BAD_NUMBER: &str = "B2C-E0310";
/// The two values of a conditional value have incompatible types.
pub(crate) const BRANCH_TYPES: &str = "B2C-E0311";
/// Text or a character value that cannot be used as a literal.
pub(crate) const BAD_LITERAL: &str = "B2C-E0312";

// --- Structure and flow (E04xx) --------------------------------------------

/// `break` or `continue` outside a loop.
pub(crate) const OUTSIDE_LOOP: &str = "B2C-E0401";
/// A `return` without the value it needs.
pub(crate) const RETURN_NEEDS_VALUE: &str = "B2C-E0403";
/// A `return` with a value in a function that gives back nothing.
pub(crate) const RETURN_HAS_VALUE: &str = "B2C-E0404";
/// No `when program starts` block.
pub(crate) const NO_MAIN: &str = "B2C-E0405";
/// More than one `when program starts` block.
pub(crate) const MANY_MAINS: &str = "B2C-E0406";
/// A function that can reach its end without returning a value.
pub(crate) const MISSING_RETURN: &str = "B2C-E0410";
/// A block that lacks a value, name or setting the analyser needs.
pub(crate) const INCOMPLETE_BLOCK: &str = "B2C-E0430";
/// Blocks nested more deeply than the analyser supports.
pub(crate) const TOO_DEEP: &str = "B2C-E0431";
/// An expression slot that does not follow the expression grammar.
pub(crate) const SLOT_SYNTAX: &str = "B2C-E0440";
/// An unfinished expression slot (a draft, or unrecognised text).
pub(crate) const SLOT_DRAFT: &str = "B2C-E0441";
/// An expression slot with too many tokens.
pub(crate) const SLOT_TOO_LONG: &str = "B2C-E0442";
/// An expression slot nested too deeply.
pub(crate) const SLOT_TOO_DEEP: &str = "B2C-E0443";

// --- Lints (W05xx / I05xx) and literal overflow ----------------------------

/// A declaration that hides another one with the same name.
pub(crate) const SHADOWING: &str = "B2C-W0501";
/// A statement that can never run.
pub(crate) const UNREACHABLE: &str = "B2C-W0502";
/// A variable used before it is given a value.
pub(crate) const USE_BEFORE_ASSIGNMENT: &str = "B2C-W0503";
/// Integer division where a decimal result is expected.
pub(crate) const INTEGER_DIVISION: &str = "B2C-W0510";
/// `==` or `!=` on decimal numbers.
pub(crate) const FLOAT_EQUALITY: &str = "B2C-W0511";
/// A `forever` loop that nothing can leave.
pub(crate) const ENDLESS_LOOP: &str = "B2C-I0513";
/// A numeric literal that does not fit its type.
pub(crate) const LITERAL_OVERFLOW: &str = "B2C-E0517";
/// A conversion that can lose information.
pub(crate) const NARROWING: &str = "B2C-W0518";
/// True/false values and numbers mixed.
pub(crate) const BOOL_NUMBER: &str = "B2C-W0519";
/// A block comment too long to keep.
pub(crate) const COMMENT_DROPPED: &str = "B2C-W0521";
/// A `for` loop step that is zero or negative.
pub(crate) const BAD_STEP: &str = "B2C-W0522";

#[cfg(test)]
pub(crate) const ALL: &[&str] = &[
    NOT_DECLARED,
    USED_BEFORE_DECLARATION,
    OUT_OF_SCOPE,
    UNAVAILABLE,
    NAME_HIDDEN,
    OTHER_MODULE,
    WRONG_KIND,
    DUPLICATE_NAME,
    DUPLICATE_FUNCTION,
    DUPLICATE_SYMBOL_ID,
    INVALID_NAME,
    CONVERSION,
    BAD_OPERANDS,
    TEXT_PLUS,
    MOD_DECIMAL,
    NO_VALUE,
    ARGUMENT_COUNT,
    EDITABLE_ARGUMENT,
    NOT_ASSIGNABLE,
    AUTO_WITHOUT_VALUE,
    BAD_NUMBER,
    BRANCH_TYPES,
    BAD_LITERAL,
    OUTSIDE_LOOP,
    RETURN_NEEDS_VALUE,
    RETURN_HAS_VALUE,
    NO_MAIN,
    MANY_MAINS,
    MISSING_RETURN,
    INCOMPLETE_BLOCK,
    TOO_DEEP,
    SLOT_SYNTAX,
    SLOT_DRAFT,
    SLOT_TOO_LONG,
    SLOT_TOO_DEEP,
    SHADOWING,
    UNREACHABLE,
    USE_BEFORE_ASSIGNMENT,
    INTEGER_DIVISION,
    FLOAT_EQUALITY,
    ENDLESS_LOOP,
    LITERAL_OVERFLOW,
    NARROWING,
    BOOL_NUMBER,
    COMMENT_DROPPED,
    BAD_STEP,
];

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::ALL;

    const REFERENCE: &str = include_str!("../../../docs/reference/diagnostics/analyser.md");

    #[test]
    fn codes_are_unique_and_well_formed() {
        let unique: BTreeSet<&str> = ALL.iter().copied().collect();
        assert_eq!(unique.len(), ALL.len(), "duplicate code");
        for code in ALL {
            let rest = code.strip_prefix("B2C-").expect("prefix");
            let (letter, digits) = rest.split_at(1);
            assert!(["E", "W", "I"].contains(&letter), "{code}");
            assert!(
                digits.len() == 4 && digits.chars().all(|c| c.is_ascii_digit()),
                "{code}"
            );
        }
    }

    #[test]
    fn every_code_is_documented() {
        for code in ALL {
            assert!(
                REFERENCE.contains(&format!("## {code}")),
                "{code} is not documented in analyser.md"
            );
        }
        // And nothing documented is unknown.
        for line in REFERENCE.lines() {
            if let Some(code) = line.strip_prefix("## B2C-") {
                let code = format!("B2C-{}", code.split_whitespace().next().unwrap_or_default());
                assert!(
                    ALL.contains(&code.as_str()),
                    "{code} is documented but never reported"
                );
            }
        }
    }
}
