//! Catalog signatures (ADR-0017 §5, ADR-0018;
//! `docs/plans/m3-contracts/types.md` §4, `slots.md` §5): every block has
//! one, in catalog order; M2's blocks carry the patterns W1 gives them; type
//! fields and parameter rows accept what their signature allows; headers
//! come from the header table; and a type or header that a newer catalog
//! may have added is `B2C-E0602` in a document from that catalog.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::needless_pass_by_value,
    reason = "test helpers fail the test by panicking and take json! values"
)]

mod common;

use std::collections::BTreeMap;

use b2c_catalog::{BlockDef, Catalog, CatalogFile, FieldKind, core_catalog, resolve};
use b2c_ir::{Diagnostic, Header, PartName, TypeClass, TypePattern, TypeTerm, TypeVar};
use b2c_model::{Block, Document, FieldValue};
use common::project;
use serde_json::{Value, json};

fn var(name: &str) -> TypeVar {
    TypeVar::new(name).unwrap()
}

fn pattern(text: &str) -> TypePattern {
    TypePattern::parse(text).unwrap()
}

/// The codes and messages of resolving a document against the core catalog.
fn problems(document: &Document) -> Vec<(String, String)> {
    pairs(&resolve(document, core_catalog()).1)
}

fn pairs(diagnostics: &[Diagnostic]) -> Vec<(String, String)> {
    diagnostics
        .iter()
        .map(|d| (d.code.0.clone(), d.message.clone()))
        .collect()
}

/// A project whose `main` holds `body`, saved by catalog `catalog`.
fn document(catalog: &str, body: Value, top: Value) -> Document {
    let mut value = project(body, top);
    value["generator"]["catalog"] = json!(catalog);
    common::load(&value)
}

/// The first statement of `main`.
fn first_step(document: &mut Document) -> &mut Block {
    document.modules[0].workspace.blocks[0]
        .statements
        .get_mut("BODY")
        .unwrap()
        .first_mut()
        .unwrap()
}

fn term(json: Value) -> TypeTerm {
    serde_json::from_value(json).unwrap()
}

#[test]
fn every_block_has_a_signature_in_catalog_order() {
    let catalog = core_catalog();
    let sigs = catalog.sigs();
    assert_eq!(sigs.len(), catalog.blocks.len());
    for (id, def) in &catalog.blocks {
        let sig = sigs.get(id).unwrap_or_else(|| panic!("{id} has no signature"));
        // The parts with a pattern: the fields as listed, then the inputs.
        let order: Vec<PartName> = def
            .field
            .iter()
            .filter(|f| f.pattern.is_some())
            .map(|f| PartName::Field(f.name.clone()))
            .chain(
                def.input
                    .iter()
                    .filter(|i| i.pattern.is_some())
                    .map(|i| PartName::Input(i.name.clone())),
            )
            .collect();
        assert_eq!(sig.order, order, "{id}");
        // Every value input, with its check class.
        assert_eq!(sig.inputs.len(), def.input.len(), "{id}");
        for input in &def.input {
            assert_eq!(sig.inputs[&input.name].check, input.check, "{id}");
            assert_eq!(sig.inputs[&input.name].assign, input.assign, "{id}");
        }
        let headers: Vec<&str> = sig.headers.iter().map(|h| h.as_str()).collect();
        assert_eq!(headers, def.headers, "{id}");
        assert_eq!(sig.output.is_some(), def.shape.is_value(), "{id}");
        // No M2 block takes its type from its place or lists members.
        assert_eq!(sig.place_var, None, "{id}");
        assert!(sig.members.is_empty(), "{id}");
        assert!(sig.option_headers.is_empty(), "{id}");
    }
}

/// `docs/plans/m3-contracts/slots.md` §5.
#[test]
fn m2_blocks_have_the_patterns_of_w1() {
    let sigs = core_catalog().sigs();
    let t = || TypePattern::Var(var("T"));
    let value = BTreeMap::from([(var("T"), TypeClass::Value)]);

    let declare = sigs.get("var.declare").unwrap();
    assert_eq!(declare.vars, value);
    assert_eq!(declare.fields["TYPE"], t());
    assert_eq!(declare.fields["NAME"], t());
    assert_eq!(declare.inputs["VALUE"].pattern, Some(t()));
    assert!(!declare.inputs["VALUE"].assign);
    assert_eq!(
        declare.order,
        [
            PartName::Field("TYPE".into()),
            PartName::Field("NAME".into()),
            PartName::Input("VALUE".into())
        ]
    );

    let set = sigs.get("var.set").unwrap();
    assert_eq!(set.vars, value);
    assert_eq!(set.fields["VAR"], t());
    assert_eq!(set.inputs["VALUE"].pattern, Some(t()));
    assert!(set.inputs["VALUE"].assign);

    for call in ["func.call", "func.call_stmt"] {
        let sig = sigs.get(call).unwrap();
        assert!(sig.vars.is_empty(), "{call}");
        assert_eq!(sig.inputs["ARG"].pattern, Some(pattern("param:FUNC")), "{call}");
        assert_eq!(sig.order, [PartName::Input("ARG".into())], "{call}");
    }
    let ret = sigs.get("func.return").unwrap();
    assert_eq!(ret.inputs["VALUE"].pattern, Some(TypePattern::Result));

    // The other blocks keep their M2 rules: no variables and no patterns on
    // their parts. M2's `symbol` output names the block's reference field.
    let patterned = [
        "var.declare",
        "var.set",
        "func.call",
        "func.call_stmt",
        "func.return",
    ];
    for (id, sig) in sigs.iter().filter(|(id, _)| !patterned.contains(id)) {
        assert!(sig.vars.is_empty() && sig.order.is_empty(), "{id}");
    }
    assert_eq!(
        sigs.get("var.get").unwrap().output,
        Some(TypePattern::Symbol(Some("VAR".into())))
    );
    assert_eq!(
        sigs.get("func.call").unwrap().output,
        Some(TypePattern::Symbol(Some("FUNC".into())))
    );
    assert_eq!(
        sigs.get("math.convert").unwrap().output,
        Some(pattern("field:TO"))
    );
    assert_eq!(
        sigs.get("text.join").unwrap().output,
        Some(pattern("std::string"))
    );
    assert_eq!(
        sigs.get("math.arithmetic").unwrap().output,
        Some(TypePattern::Number)
    );
}

/// `docs/plans/m3-contracts/types.md` §5.3: four core blocks name only the
/// headers all their forms need, which the generator always emits with them.
#[test]
fn core_blocks_name_only_the_headers_every_form_needs() {
    let headers = |id: &str| core_catalog().blocks[id].headers.clone();
    assert!(headers("text.literal").is_empty());
    assert!(headers("program.exit").is_empty());
    assert!(headers("math.random_int").is_empty());
    assert_eq!(headers("io.ask"), ["<iostream>"]);
    assert_eq!(headers("io.print"), ["<iostream>"]);
    assert_eq!(headers("text.join"), ["<string>"]);
    for def in core_catalog().blocks.values() {
        for header in &def.headers {
            assert!(Header::standard(header).is_some(), "{}: {header}", def.id);
        }
    }
}

/// A catalog made of the core catalog and one more definition.
fn with_block(toml: &str) -> Catalog {
    let file: CatalogFile = toml::from_str(toml).unwrap();
    let mut blocks = core_catalog().blocks.clone();
    for def in file.block {
        blocks.insert(def.id.clone(), def);
    }
    Catalog::new(core_catalog().version.clone(), blocks)
}

/// `coll.item` with an input type and its keys.
fn item_block(index: &str) -> String {
    format!(
        "[[block]]\nid = \"coll.item\"\nversion = 1\ncategory = \"variables\"\nshape = \"reporter\"\n\
         output = \"elem(C)\"\nwhere = {{ C = \"indexable\" }}\n\
         label = {{ friendly = \"item %INDEX of %LIST\", cpp = \"%LIST.at(%INDEX)\" }}\n\
         lowering = \"builtin\"\nhelp = \"An item.\"\n\
         [[block.field]]\nname = \"LIST\"\nkind = \"symbol_ref\"\ntype = \"C\"\n\
         [[block.input]]\nname = \"INDEX\"\ncheck = \"any\"\n{index}\n"
    )
}

#[test]
fn broken_signatures_are_broken_definitions() {
    let use_item = || {
        document(
            "1.3.0",
            json!([{"id": "b1", "type": "io.print", "v": 1, "inputs": {"ITEM0": {"block":
                {"id": "b2", "type": "coll.item", "v": 1, "fields": {"LIST": {"ref": "s_list"}},
                 "inputs": {"INDEX": {"expr": [{"num": "0"}]}}}}}}]),
            json!([]),
        )
    };
    // A good signature has one and resolves the block.
    let good = with_block(&item_block("type = \"index(C)\""));
    assert!(good.sigs().get("coll.item").is_some());
    assert_eq!(pairs(&resolve(&use_item(), &good).1), []);
    // A default on `index(C)` (types.md §4.3), a variable nothing binds, an
    // undeclared variable: the definition is broken, gets no signature, and
    // its blocks are reported once and kept unchecked.
    for (keys, problem) in [
        (
            "type = \"index(C)\"\ndefault = [{ num = \"0\" }]",
            "input INDEX has a default, but its type index(C) is not one fixed type",
        ),
        (
            "type = \"index(K)\"",
            "uses the type variable K, which where does not declare",
        ),
        ("type = \"int\"\nassign = true", ""),
    ] {
        let catalog = with_block(&item_block(keys));
        let diagnostics = resolve(&use_item(), &catalog).1;
        if problem.is_empty() {
            assert_eq!(pairs(&diagnostics), [], "{keys}");
            continue;
        }
        assert!(catalog.sigs().get("coll.item").is_none(), "{keys}");
        assert_eq!(diagnostics.len(), 1, "{keys}: {diagnostics:?}");
        assert_eq!(diagnostics[0].code.0, "B2C-E0620");
        assert!(
            diagnostics[0].message.contains(problem),
            "{}",
            diagnostics[0].message
        );
    }
    // A pattern that does not parse makes its file invalid, like any other
    // value of the wrong shape.
    let toml = item_block("type = \"index(C\"");
    let error = toml::from_str::<CatalogFile>(&toml).unwrap_err().to_string();
    assert!(error.contains("is not a type pattern"), "{error}");
}

#[test]
fn create_takes_every_type_a_variable_can_hold() {
    let declare = |ty: Value| {
        json!([{"id": "b1", "type": "var.declare", "v": 1,
                "fields": {"TYPE": ty, "NAME": {"sym": "s_x", "name": "x"}}}])
    };
    for ty in ["int", "double", "std::string", "auto"] {
        assert_eq!(
            problems(&document("1.3.0", declare(json!(ty)), json!([]))),
            [],
            "{ty}"
        );
    }
    // A user type: the analyser checks the reference (E0201, E0207).
    assert_eq!(
        problems(&document("1.3.0", declare(json!({"ref": "s_point"})), json!([]))),
        []
    );
    // A template term (format version 2), as the loader will read it.
    let mut doc = document("1.3.0", declare(json!("int")), json!([]));
    let list = term(json!({"std::map": ["std::string", {"std::vector": "int"}]}));
    first_step(&mut doc)
        .fields
        .insert("TYPE".into(), FieldValue::Type(list));
    assert_eq!(problems(&doc), []);
    // Not a value type, not a type of this catalog, not a type: E0607.
    let wider = "\"int\", \"double\", \"bool\", \"char\", \"std::string\" or \"auto\", or another type a variable can hold.";
    for (ty, shown) in [
        ("std::ifstream", "\"std::ifstream\""),
        ("void", "\"void\""),
        ("std::span", "\"std::span\""),
    ] {
        assert_eq!(
            problems(&document("1.3.0", declare(json!(ty)), json!([]))),
            [(
                "B2C-E0607".to_owned(),
                format!("The field TYPE is {shown}, but it must be one of the types {wider}")
            )],
            "{ty}"
        );
    }
    let mut doc = document("1.3.0", declare(json!("int")), json!([]));
    let streams = term(json!({"std::vector": "std::ofstream"}));
    first_step(&mut doc)
        .fields
        .insert("TYPE".into(), FieldValue::Type(streams));
    assert_eq!(
        problems(&doc),
        [(
            "B2C-E0607".to_owned(),
            format!(
                "The field TYPE is \"{{\\\"std::vector\\\":\\\"std::ofstream\\\"}}\", but it must be one of the types {wider}"
            )
        )]
    );
}

#[test]
fn fields_without_a_pattern_take_only_their_types() {
    let convert = |to: Value| {
        json!([{"id": "b1", "type": "io.print", "v": 1, "inputs": {"ITEM0": {"block":
            {"id": "b2", "type": "math.convert", "v": 1, "fields": {"TO": to}}}}}])
    };
    assert_eq!(
        problems(&document("1.3.0", convert(json!("double")), json!([]))),
        []
    );
    assert_eq!(
        problems(&document("1.3.0", convert(json!({"ref": "s_point"})), json!([]))),
        [(
            "B2C-E0607".to_owned(),
            "The field TO is a struct or enum, but it must be one of the types \"int\" or \"double\"."
                .to_owned()
        )]
    );
    assert_eq!(
        problems(&document("1.3.0", convert(json!("char")), json!([]))),
        [(
            "B2C-E0607".to_owned(),
            "The field TO is \"char\", but it must be one of the types \"int\" or \"double\".".to_owned()
        )]
    );
}

/// A function definition with these parameter rows.
fn define(rows: Value) -> Value {
    json!([{"id": "b_f", "type": "func.define", "v": 1,
            "fields": {"NAME": {"sym": "s_f", "name": "f"}}, "extra": {"params": rows}}])
}

fn row(sym: &str, ty: Value) -> Value {
    json!({"sym": sym, "name": sym, "type": ty, "mode": "read_only"})
}

#[test]
fn parameters_take_every_type_a_variable_can_hold() {
    for ty in [
        json!("int"),
        json!("double"),
        json!({"std::vector": "int"}),
        json!({"std::map": ["int", {"std::set": "std::string"}]}),
        json!({"ref": "s_point"}),
    ] {
        assert_eq!(
            problems(&document(
                "1.3.0",
                json!([]),
                define(json!([row("s_p", ty.clone())]))
            )),
            [],
            "{ty}"
        );
    }
    let wider =
        "\"int\", \"double\", \"bool\", \"char\" or \"std::string\", or another type a variable can hold.";
    for (ty, shown) in [
        (json!("std::ifstream"), "\"std::ifstream\""),
        (json!("auto"), "\"auto\""),
        (json!("long"), "\"long\""),
        (json!({"std::map": ["int"]}), "an object"),
        (json!(3), "3"),
        (
            json!({"std::vector": "void"}),
            "\"{\\\"std::vector\\\":\\\"void\\\"}\"",
        ),
    ] {
        assert_eq!(
            problems(&document(
                "1.3.0",
                json!([]),
                define(json!([row("s_p", ty.clone())]))
            )),
            [(
                "B2C-E0614".to_owned(),
                format!("Parameter 1 of this block has the type {shown}, but parameters can only be {wider}")
            )],
            "{ty}"
        );
    }
}

#[test]
fn a_newer_catalog_may_add_types_and_widen_fields() {
    let newer = "1.4.0";
    let declare = |ty: Value| {
        json!([{"id": "b1", "type": "var.declare", "v": 1,
                "fields": {"TYPE": ty, "NAME": {"sym": "s_x", "name": "x"}}}])
    };
    let newer_block = |what: &str| {
        (
            "B2C-E0602".to_owned(),
            format!(
                "This \"create\" block was made with a newer version of Blocks2Cpp: it has {what}, which this version does not know. Update Blocks2Cpp to use it; until then the block is kept as it is."
            ),
        )
    };
    // A type this catalog does not know.
    assert_eq!(
        problems(&document(newer, declare(json!("std::span")), json!([]))),
        [newer_block("the type \"std::span\" in its field \"TYPE\"")]
    );
    // A known type outside this catalog's domain: the newer one may have widened it.
    assert_eq!(
        problems(&document(newer, declare(json!("std::ifstream")), json!([]))),
        [newer_block("the type \"std::ifstream\" in its field \"TYPE\"")]
    );
    // What no catalog writes stays a broken field.
    assert_eq!(
        problems(&document(newer, declare(json!("int; x")), json!([])))
            .into_iter()
            .map(|(code, _)| code)
            .collect::<Vec<_>>(),
        ["B2C-E0607"]
    );
    // The same in a parameter row, and with a template.
    let rows = define(json!([
        row("s_p", json!("int")),
        row("s_q", json!({"std::optional": "int"}))
    ]));
    let found = problems(&document(newer, json!([]), rows));
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, "B2C-E0602");
    assert!(
        found[0]
            .1
            .contains("it has the type \"{\\\"std::optional\\\":\\\"int\\\"}\" in parameter 2"),
        "{}",
        found[0].1
    );
    // From this catalog, they are what they are.
    assert_eq!(
        problems(&document("1.3.0", declare(json!("std::span")), json!([])))[0].0,
        "B2C-E0607"
    );
}

/// `use header` (types.md §5.4) is W2's; its field kind is checked now.
#[test]
fn header_fields_take_standard_headers() {
    let toml = "[[block]]\nid = \"org.use_header\"\nversion = 1\ncategory = \"program\"\nshape = \"definition\"\n\
                label = { friendly = \"use header %HEADER\", cpp = \"#include %HEADER\" }\nlowering = \"builtin\"\n\
                help = \"Includes a standard header.\"\n\
                [[block.field]]\nname = \"HEADER\"\nkind = \"header\"\ndefault = \"<vector>\"\n";
    let catalog = with_block(toml);
    let def: &BlockDef = &catalog.blocks["org.use_header"];
    assert_eq!(def.field[0].kind, FieldKind::Header);
    assert!(catalog.sigs().get("org.use_header").is_some());
    let use_header = |catalog_version: &str, header: Value| {
        document(
            catalog_version,
            json!([]),
            json!([{"id": "b1", "type": "org.use_header", "v": 1, "fields": {"HEADER": header}}]),
        )
    };
    let codes =
        |doc: &Document| -> Vec<String> { resolve(doc, &catalog).1.into_iter().map(|d| d.code.0).collect() };
    assert!(codes(&use_header("1.3.0", json!("<map>"))).is_empty());
    for bad in [
        json!("<bits/stdc++.h>"),
        json!("<newthing>"),
        json!("map"),
        json!(true),
    ] {
        assert_eq!(codes(&use_header("1.3.0", bad.clone())), ["B2C-E0607"], "{bad}");
    }
    let message = &resolve(&use_header("1.3.0", json!("map")), &catalog).1[0].message;
    assert_eq!(
        message,
        "The field HEADER is \"map\", but it must be a standard header such as \"<vector>\"."
    );
    // A header of a newer table, in a document from a newer catalog.
    assert_eq!(codes(&use_header("1.4.0", json!("<newthing>"))), ["B2C-E0602"]);
    assert_eq!(
        codes(&use_header("1.4.0", json!("<bits/stdc++.h>"))),
        ["B2C-E0607"]
    );
    // Its default must be in the table.
    let broken = with_block(&toml.replace("<vector>", "<vector.h>"));
    assert!(broken.sigs().get("org.use_header").is_none());
}

#[test]
fn dropdown_options_may_need_their_own_headers() {
    let toml = "[[block]]\nid = \"math.fn\"\nversion = 1\ncategory = \"math\"\nshape = \"reporter\"\noutput = \"double\"\n\
                label = { friendly = \"%FN of %X\", cpp = \"%FN(%X)\" }\nlowering = \"builtin\"\nhelp = \"A function.\"\n\
                [[block.field]]\nname = \"FN\"\nkind = \"dropdown\"\noptions = [[\"square root\", \"sqrt\"], [\"larger of\", \"max\"]]\n\
                default = \"sqrt\"\nheaders = { sqrt = [\"<cmath>\"], max = [\"<algorithm>\"] }\n\
                [[block.input]]\nname = \"X\"\ncheck = \"number\"\ndefault = [{ num = \"1\" }]\n";
    let catalog = with_block(toml);
    let sig = catalog.sigs().get("math.fn").unwrap();
    assert_eq!(
        sig.option_headers["FN"]["sqrt"],
        [Header::standard("<cmath>").unwrap()]
    );
    assert_eq!(
        sig.option_headers["FN"]["max"],
        [Header::standard("<algorithm>").unwrap()]
    );
    // Headers for something that is not an option break the definition.
    let broken = with_block(&toml.replace("max = [", "min = ["));
    assert!(broken.sigs().get("math.fn").is_none());
}
