# M3 contract deviations and the owner's decisions

**Decided by the owner on 2026-11-27:** every Strong row is accepted as
built; the five Unclear rows (W3-5, W3-7, W3-13, W3-18, W3-21) are
accepted as built; every Weak row is reworked to its contract, in the
packages of [next-steps §3.10](../next-steps.md#310-what-remains-of-m3)
item 3. W5P-4 was already done (`a5f1af8`, `0e328eb`).

Each row is a place where a package built something other than what its contract (or proposal) said. Decide for each one: **accept as built** or **rework to the contract**.

Ratings:

- **Strong:** the reason is correctness, C++ or safety, or the contract was wrong about C++.
- **Weak:** the reason is cost, convenience or simplicity. Each Weak row has a rework size: S, M or L.
- **Unclear:** the stated reason does not settle it.

Paths are relative to `docs/plans/m3-contracts/`. "Ref" gives the source line or item.

W4's rows that the owner has already decided (marked *Accepted by the owner* or *Reworked to the contract*) are left out. The one unmarked W4 row is W4-1.

---

## w2.md §7.1 (60 rows: 32 included as 27 IDs, 28 excluded)

| ID | Contract said | Built | Stated reason | Rating |
|---|---|---|---|---|
| W2-1 (l.1328) | A `case` label is a narrower kind of constant | A label may be any constant expression of whole numbers and characters. P0 checks it in `consts.rs`. | C++ accepts any integral constant expression, and only `consts.rs` knows the `const` values | Strong |
| W2-2 (l.1335) | The collection blocks are in the benchmark workspaces (webview included) | The blocks are only in a separate native benchmark shape (`preview-collections`), which is reported but not gated | The A/B job reads with the base build's catalog, which refuses them | Strong |
| W2-3 (l.1337) | The block ID is `math.fn2` | The block ID is `math.fn_two` | The catalog rule refuses digits in IDs (`E0620`) | Strong |
| W2-4 (l.1339) | `text.as_text`'s `VALUE` takes a `printable` pattern | It takes `check = "any"`, as *join* does | Both blocks must refuse the same types with the same message; the pattern would let refused structs through | Strong |
| W2-5 (l.1342) | The toolbox has a *new Student with …* item for each struct | Only the static `type.new` with `fieldCount` 0, and no item for each struct | The toolbox inflater labels only `ARG` parts, and C1 did not edit that file | **Weak, M** |
| W2-6 (l.1346) | *for each line* declares `std::string line;` | It declares `std::string line{};` | Every declaration is value-initialised the same way (consistency); the C++ behaves the same | **Weak, S** |
| W2-7 (l.1348) | A non-number `case` label gets `E0424` | It gets `E0301` once, and not `E0424` as well | Avoids reporting the same problem twice | Strong |
| W2-8 (l.1351) | `param:` and `member:` may name the block's own declaration | Only on an input that the `params` or `fields` rows repeat; part *i* takes row *i*'s type | Those rows are the only parameters or fields such an input can mean | Strong |
| W2-9 (l.1352) | Default values are lowered with the function | They are lowered after every function is declared, with no parameter in scope (`E0203`). A new `optional` count is added. | A default may call any function; C++ scoping | Strong |
| W2-10 (l.1353) | An `&` parameter may have any default | The default must be a changeable variable (`E0307`) | `int& x = 1` does not compile | Strong |
| W2-11 (l.1354) | Functions are printed in the flat order | Function declarations are nodes of the declaration graph and are printed in `source_order`, with forward declarations | `void f(int n = g())` needs `g` declared first | Strong |
| W2-12 (l.1355) | §2.8's overload rule applies | Same-named functions in different modules stay `E0211` (their own text); the overload rule applies within a module | Modules are compiled separately until W3 | Strong |
| W2-13 (l.1356) | Overload casts and `std::string(…)` on calls | Printed only on calls to a name that is shared within the module | Keeps the golden files byte-identical; a single candidate needs no help | **Weak, S** |
| W2-14 (l.1358) | A *local* named like a global is `W0501` | Parameters and catch names are covered too; globals are unique program-wide (`E0210`) | They are locals too; modules would not link otherwise | Strong |
| W2-15 (l.1361) | `var.global`'s `TYPE` includes `auto` | M2's five types plus the interim list types, without `auto` | A global's type is printed before any value is lowered (an implementation order) | **Weak, M** |
| W2-16 (l.1363) | *change*'s bitwise forms take "whole numbers" | An `int` variable only. Note: a constant shift amount that is negative or ≥32 is not reported. | `char c; c <<= 1;` fails `-Wconversion` at the strict level | Strong (shift-check part unclear) |
| W2-17 (l.1368) | A number condition in *check at compile time* is `E0301` | It is `W0519`, as for *if*; the message is `E0312` when it cannot be a string literal | C++ converts the number | Strong |
| W2-18 (l.1370) | An overloaded call prints a *list of* argument as `{…}` | It prints the type: `show(std::vector<int>{1})` | `show({1})` picks `show(int)`; `sum({1, 2})` is ambiguous | Strong |
| W2-19 (l.1371-1374, 1377-1378) | These programs were accepted | Six new refusals: (a) a default value naming an input `E0205`, (b) a self-calling default `E0421`, (c) a struct field default naming a field `E0205`, (d) a C++17 gap in *new … with* before a defaulted field `E0430`, (e) `main` local named `ARGV` `E0210`, (f) a variable or field named like a struct or enum `E0210`, *open file*'s path naming the stream `E0205`/`E0202` | Each case fails to compile, or silently does something else in C++ | Strong |
| W2-20 (l.1376) | Icons ⑂, 🗎 and 𝑥 | Icons ⋔, ▤ and x, enforced by a font-coverage test | DejaVu Sans drew the old icons as empty boxes, and the icon is the non-colour cue | Strong |
| W2-21 (l.1379) | add, remove and clear on a list are allowed anywhere | Inside a *for each* over the same variable they are `E0308` | Undefined behaviour (ASan use-after-free), silent on MinGW | Strong |
| W2-22 (l.1380) | A *copy* item is `auto x` | A list of `bool` gives `bool x` | `auto` on `vector<bool>` deduces a proxy, so the "copy" changes the list | Strong |
| W2-23 (l.1381) | C++20 *sort* is `std::ranges::sort` | A list of `bool` uses `std::sort(begin, end)` | `vector<bool>` is not `sortable` before C++23 | Strong |
| W2-24 (l.1382) | `groups.insert({})` | `groups.insert(std::vector<int>{})` | `{}` binds to the `initializer_list` overload and inserts nothing | Strong |
| W2-25 (l.1383) | *for each line* and *read line* use bare `std::getline` | They use the `b2c::read_line` helper, which strips `'\r'` | Files with CRLF endings gave wrong answers on Linux and macOS | Strong |
| W2-26 (l.1384) | Type fields list the program's types | Only the shown module's types; another module's type is `E0206` | Each module is its own `.cpp`, so only g++ caught it | Strong |
| W2-27 (l.1387) | Category colours follow §2.1's lightness rule | Types and Files are darker and less saturated; a test keeps every pair ≥6 apart in OKLab | At the rule's lightness the colours were indistinguishable (3.1 vs M2's 6.3) | Strong |

**Excluded (28):** these record a package or file owner (rows 1, 4, 9, 11, 19, 24, ...), a contract text or file-name error (`emit.rs`), scheduling (`W0508`, the *Message* lines), detail the contract left open (`userTypes()`, `W0507` texts, labels, the menu filter, struct-row defaults, `input_label`, the toolbox key, the scope of definition blocks, the `read line` condition form, *flip*, `main`'s label), internal order or mechanism with no visible change (the C1 pre-pass, the `b2c_file` naming, `<cmath>` placement), a bug fix (the double warning), or a row since superseded (the `constexpr double` globals, reworked by W4; the per-global *initialised* walk, replaced by W3 T's graph).

---

## w3-modules.md §7.1 and "From the W4 and W5 merges" (74 rows: 24 included, 50 excluded)

| ID | Contract said | Built | Stated reason | Rating |
|---|---|---|---|---|
| W3-1 (l.1377) | Casts when any function of the program shares the name | Only when the functions this module can call share it | Two modules' non-shared functions are not an overload set | Strong |
| W3-2 (l.1378) | The `::name` guard qualifies the call | The plain name is printed for an anonymous-namespace function that an included header also declares, so C++ refuses the call | Never silently calls the other module's function | Strong |
| W3-3 (l.1383) | Status messages go to an app-wide region | They go to the module strip's own polite status region | The app has no window-wide status region | **Weak, S** |
| W3-4 (l.1384) | `AnalysisState.scope: ScopeIndex \| null` (W5 §10.3) | `ScopeIndex \| 'none' \| null` | With `null` the main core answers for a document it has not seen | Strong |
| W3-5 (l.1386) | The unchecked-change refusal is shown in another form | An alert dialog titled *The modules cannot be changed now* | "One accessible place for the text" | Unclear |
| W3-6 (l.1394) | Headers forward-declare types they use only in lists | They include the other header, so mutual list-holding shared structs are `E0422` | `std::vector<T>{}` fields need `T` complete; the header would not compile alone | Strong |
| W3-7 (l.1396) | Uniqueness rules across modules | Globals, structs and enums are unique per module, not program-wide; new `E0213` and `E0210` texts | "The rules of §2.1 for every kind" | Unclear |
| W3-8 (l.1399) | `E0424` names the cause (another module's `extern const`) | It keeps its generic message; the reference explains the cause | Naming it would need module and value in L10's check (cost) | **Weak, S** |
| W3-9 (l.1404) | The E2E flow builds the program block by block | It opens a copy of `gradebook` | Building needs the mutator dialog (convenience) | **Weak, S** |
| W3-10 (l.1405) | Cache temporaries get temporary names in `obj/` | They go in `tmp/<stem>.o` and are renamed into `obj/` | GCC 13 and 14 name SARIF files after the output stem | Strong |
| W3-11 (l.1406) | Pruning `obj/` is part of the build | Best effort: a cache file that cannot be deleted is logged, not fatal; the messages file gets a format tag | Never fail a build that has linked | Strong |
| W3-12 (l.1408) | The namespace depth limit refuses everywhere | Drags are refused; a deeper file still loads, and the analyser reports `E0219` | A refusal while loading would turn blocks into placeholders | Strong |
| W3-13 (l.1415) | 06 §6.14.6: `std` as an associated namespace qualifies calls; `E0220`'s `stoi` part is replaced | `std` qualifies no call in M3; `E0220` keeps the `stoi` part | User functions win anyway; qualifying changed existing output | Unclear |
| W3-14 (l.1416) | The M3 lookup model covers block and function scopes | It does not: every hidden name is still `E0205` (06 allows this until the model lands) | Spellings are per block; locals are checked by the lowering | **Weak, M** |
| W3-15 (l.1417) | Textbook style nominates `std` where the directive is written | The analyser counts `std` as nominated in every `.cpp` (spellings, `E0215`, `I0523`) | The analyser does not know which headers codegen includes | **Weak, M** |
| W3-16 (l.1431) | N2 regression tests use ADR-0006's case programs | The cases were rebuilt from blocks | The originals were never committed | Strong |
| W3-17 (l.1432) | 06 §6.14.13: open each standard namespace with a statement block | Not built; it waits for *use namespace … here* (package S, cut) | Budget (S was cut) | **Weak, M** |
| W3-18 (l.1433) | Textbook style switch (W3M-D13) | A checkbox in the settings page's *Language* draft, applied with *Save changes*; no IPC change | The page saves every setting as one edit | Unclear |
| W3-19 (l.1435) | The `namespaces` E2E flow builds through the UI | It uses a fixture and the test hook | The hook cannot put definitions into a mouth (convenience) | **Weak, S** |
| W3-20 (l.1437) | Paste records every reference with its namespace | A global is recorded by plain name and a namespace by nothing (*use namespace* keeps its ID) | The clipboard format v2 has no kind for them | Strong |
| W3-21 (l.1438) | W3M-D12 (b) keeps the IDs of pasted *definitions* | Keeps the IDs of every free symbol, locals too; spec 05 §5.12 changed | One rule; a free ID collides with nothing | Unclear |
| W3-22 (l.1440) | The 8-module native benchmark is gated | `pipeline/preview-modules` is reported only, until it has baselines | The gate needs five baselines | Strong |
| W3-23 (l.1441) | The scope query lets another module's shared `geo::count` through beside an own `::count` | Hidden whatever the namespaces; the paste binds only what is listed. **Left as is.** | The fix touches `ScopeIndex::visible` and both lookups, outside the clipboard | **Weak, M** |
| W3-24 (l.1442) | Multi-module fuzz seeds go in the dictionary | They are corpus files | libFuzzer dictionary entries are at most 64 bytes | Strong |

**Excluded (50):** these record a package or file owner or extra files (`Namespace` kind, N1's files, P0 tests, `std_names` tool), internal representation or mechanism with no visible change (`spellings` table, `Layout::flat` stems, the one `shared` list (§3.3 left the shape open), checker rule versus check string, the a11y name source, the `readFields` placement, `Spellings::directives`, lookup keys, program graph replacing F's), interim steps later completed by T or N2 (header contents, `E0214`, F's `E0421` graph, the *use namespace* menu), added messages and detail (`E0206`, `E0213` and `E0223` texts, `E0215` placement, the picker groups, the Types category relabel, strip behaviours, handle methods, `CodePanel`), owner decisions (W3M-D10, the W4 §11 row-8 recommendation), contract-required behaviour (06's `::geo::area`, per-namespace uniqueness, the directive include), bug fixes after review (drag-out heal, focus, namespace follow, the global type spelling), tests, examples and fuzz plumbing, and the removed `HelperPlacement`, which §1.2 implies.

In "From the W4 and W5 merges", E's `'none'` is W3-4. M3-4 was done later (see below). The W5 §10.2 clipboard arguments were "not triggered" because their benchmarks were never added; that is W5P-3.

---

## modules.md (1 included, 1 excluded)

| ID | Contract said | Built | Stated reason | Rating |
|---|---|---|---|---|
| MOD-1 (§7.3) | *use header of module* (`program.use_module`, `E0607`) in W3 | Not built; moved to M4. It belonged to the if-time sweep (package S), which was cut as a whole. | Budget | **Weak, M** |

**Excluded (1):** §8 Export. Its first draft was superseded by the w3-export-run.md contract, which was built.

---

## w3-export-run.md §13 (1 included, 1 excluded)

| ID | Contract said | Built | Stated reason | Rating |
|---|---|---|---|---|
| X-1 ("Not done") | Coverage for *End input*, the input file and the external terminal | No E2E flow: Rust and Vitest tests only. "Can follow with W5's sweep." | Coverage judged enough (cost) | **Weak, S** |

**Excluded (1):** the "known limits" (the Windows window closing, an input file shared by two copies, POSIX make quoting) are platform limits, not changes to the contract. The 17 review findings are bug fixes.

---

## w4-editor-tools.md §7.1 (the unmarked row)

| ID | Contract said | Built | Stated reason | Rating |
|---|---|---|---|---|
| W4-1 (l.478, §7) | A context-menu item *Use unchecked `[]`* toggles `CHECKED`, shown as `[]` vs `.at()` in C++ labels | A hidden checkbox `CHECKED` with new catalog keys `menu` and `mark`. The menu item reads *Fast unchecked access ([] instead of .at())* and the block shows *unchecked*. (The C++-label part was already accepted, row l.485.) | Keeps the friendly-label rule and the menu generic | **Weak, S** |

---

## w4-appearance-expert.md §10 (0 included, 2 excluded)

**Excluded (2):** the §6 work was "not done" only until W2 integrated, and the coordinator did it later (§6 note). W4A-4 and W4A-5 are open owner decisions, not deviations.

---

## worker.md as refined by w5-worker.md §9 (3 included, 5 excluded)

| ID | Contract said | Built | Stated reason | Rating |
|---|---|---|---|---|
| W5W-1 (item 1) | The probe's worker request reports `blocked` | The probe tries both IPC endpoints, with a page request as control. On WebKitGTK it can only say `blocked` or `failed`. | WebKitGTK raises no `securitypolicyviolation` in workers | Strong |
| W5W-2 (item 3) | `rollupOptions`; the build test expects exactly one file under `workers/` | `worker.rolldownOptions`; the worker is two files; the test checks for exactly one *entry* | Vite 8's names; the glue is a dynamic chunk | Strong |
| W5W-3 (item 5) | Client API; failure after three starts | Adds `acceptVersion` and `log`. A `DataCloneError`, an uncreatable worker or a failed compile fails at once. The scope is `scopeJson: string \| null`. | Those failures cannot recover; `ScopeIndex` did not exist yet | Strong |

**Excluded (5):** item 2 (a fact: Trusted Types on WebKitGTK), item 4 (an indirect `eval` with the same effect), item 6 (extra keys rejected: detail), item 7 (the apply rule in two halves: phasing, completed in W5's second part), and item 8 (a timer granularity method; its copy, paste and duplicate deferral is folded into W5P-3).

---

## w5-performance.md §10.1 M3-4: the nine corrections to the proposal (7 included, 2 excluded)

| ID | Proposal said | Built | Stated reason | Rating |
|---|---|---|---|---|
| M34-1 | A side table `parked_symbols`; `insert_symbol` picks the table by `ItemCtx` | Inserted into the program's table, then moved into `Parked::symbols` after all stacks | About 100 reads of `self.symbols`; a parked variable would have no type | Strong |
| M34-2 | `parked_symbols` and `parked_stmts` fields | One field `Analysis.parked {symbols, stmts, values}`; a loose reporter or predicate is lowered as a value for `block_types` | The test needs parked reporters, and a reporter is not a statement | Strong |
| M34-3 | `with_members` and `members` also read the parked table | Unchanged; only `infos`, `scope_after`, `created` and `symbol_infos` read both | Parked stacks declare only variables | Strong |
| M34-4 | A parked `return` is typed as in `main` | Typed against `Unknown`, with no report | The attach point is unknown; `main`'s `int` would mistype list values | Strong |
| M34-5 | The shared `lower_list` | A loop like `body_and_end` in its own `#parked` frame above `#seed` | There is no `lower_list`; the own frame makes a parked `create` shadow | Strong |
| M34-6 | Parked heads come from `program()`'s catch-all arm | Taken from each module's `workspace.blocks` with `DEFINITION_TYPES` | Since W3, `sorted_blocks` reaches into namespace mouths | Strong |
| M34-9 | Four tests, including an E2E check that text no longer snaps into a parked `int` *set* | The other tests were added or updated; **the E2E check was not added** | "The E2E check is the editor's" (ownership) | **Weak, S** |

**Excluded (2):** M34-7 (`slot_types` kept; the proposal left it open) and M34-8 (diagnostics swapped once rather than per stack, with the same result).

---

## w5-performance.md §11 and §14: worker.md as built (5 included, 5 excluded)

| ID | Contract said | Built | Stated reason | Rating |
|---|---|---|---|---|
| W5P-1 (§11.3) | The hold ends with a `release()` call from the session | No `release()`; the pipeline re-checks `isBusy()` every animation frame | No call needed from the session (simplicity) | **Weak, S** |
| W5P-2 (§11.5) | The worker flag is removed | It is inverted and kept as an off switch (`B2C_PREVIEW_WORKER=0`) | A/B and developer measurements | **Weak, S** |
| W5P-3 (§10.2, §14; w5-worker item 8) | Clipboard arguments (worker.md §5) and the `webview.copy/paste/duplicate-1000.max-task` benchmarks (N12, 100 ms) | Neither built. A copy, paste or duplicate costs one main-thread analysis while the worker previews; the condition was never measured. | Wait for the measurement (cost) | **Weak, M** |
| W5P-4 (§14) | `webview.preview-1000.p95` is redefined as the `b2c:preview` span, plus a new `update-p95` metric | The old metric is kept, and spans give `update()` | Nightly compares like with like | **Weak, S** |
| W5P-5 (§14) | E2E additions of worker.md §2.5 | The package's worker test checks each example's golden C++; no new E2E flow | Coverage judged equivalent | **Weak, S** |

**Excluded (5):** §11.1 (format detail added), §11.2 (an interim step, then done by E: W3-4), §11.4 (an epoch mechanism, internal), the hold wiring (done later by W3 E), and lazy first-showing drawing (conditional in §3; the condition was not met).

---

## types.md, slots.md, worker.md (own text)

None of these has its own correction rows. worker.md's corrections are proposed in w5-worker.md §9 and w5-performance.md §11, which are covered above.

---

## Totals

| Rating | Rows |
|---|---|
| Strong | 43 (W2 23, W3 11, W5W 3, M34 6) |
| Weak | 21 (W2 4, W3 8, MOD 1, X 1, W4 1, M34 1, W5P 5) |
| Unclear | 5 (all W3) |
| **Included** | **69 IDs** (W2-19 groups 6 table rows, so 74 source rows) |

Weak rows by rework size: S 13, M 8, L 0.

| File | Excluded rows |
|---|---|
| w2.md | 28 |
| w3-modules.md | 50 |
| modules.md | 1 |
| w3-export-run.md | 1 |
| w4-appearance-expert.md | 2 |
| w5-worker.md | 5 |
| w5-performance.md | 7 (M3-4 2, §11 and §14 5) |
| types.md, slots.md, worker.md | 0 |
| **Total** | **94** |
