# ADR-0006: `using namespace`: a project setting and a block, with exact name rules

* Status: Accepted
* Date: 2026-10-04

## Context

Generated C++ writes standard-library names in full (`std::cout`), and new
projects keep doing so (decision Q4 in [10 §10.3](../spec/10-roadmap.md#103-product-decisions)).
Many courses and textbooks write `using namespace std;` instead, so the owner
wants both a **project setting** that does this for every file and the
**`use namespace` block** for one module or part of a function. The two must
work together, and "global" (project-wide and file-wide) and "local" (inside a
function) namespaces must interact correctly.

A using-directive changes how C++ looks names up, so it can break or silently
change a program:

* A user name that is also a standard name becomes ambiguous: a global `count`
  next to `std::count`, an enum value `left` next to `std::left`.
* A user function can silently lose to a better-matching standard template:
  with `using namespace std;`, the user's `int max(int, int)` called as
  `max(1.5, 2.5)` runs `std::max<double>`.
* In multi-module projects, non-shared names live in an anonymous namespace,
  and `::count` does not reach them when the directive is at global scope.

Of 179 common learner identifiers, 76 to 87 are standard names (`count`,
`max`, `size`, `list`, `left`, `data`, `sort`, `next`…), so any rule that simply
forbids them would make the setting painful to turn on.

## Options considered

Three independent design studies each built a complete rule set and proved it
with compiled programs (114, 88 and 127 cases on g++ 11–14, C++17/20/23).
Three adversarial reviewers then attacked the merged design with another 200+
programs, also on clang with libstdc++ and libc++, and a second round attacked
the revised rules with about 100 more.

1. **Reject every clashing name.** The generated code looks exactly like a
   textbook, and the generator stays simple. But turning the setting on forces
   renames of many common names, and functions in user namespaces must avoid
   standard names because of argument-dependent lookup.
2. **Qualify every clashing name.** No renames are ever needed: the generator
   writes `::count`, `::max(…)`, `Side::left` and also `::list` for types. Type
   names need fragile spellings (out-of-line definitions such as
   `auto ::array::origin() -> Point`), and GCC resolves some of them silently to
   the standard entity (class template argument deduction picks `std::pair` for
   the user's `pair`).
3. **Per kind of name (chosen).** Functions, variables, constants and
   enumerators are qualified (`::max(…)`, `::count`, `Side::left`), so common
   names keep working. Namespace-scope types and namespaces that clash are
   renamed with a one-click quick fix (`list` becomes `List`). Standard names
   are shortened only where lookup provably finds them.

## Decision

Option 3, specified in [06 §6.14](../spec/06-compiler-pipeline.md#614-standard-names-and-using-namespace):

* Textbook style (`options.usingNamespaceStd`, off by default) and two block
  forms (`use namespace [N] in this file`, `use namespace [N] here`).
* One lookup model in the analyser decides the spelling of every reference,
  with and without directives, so turning the setting off can never remove a
  qualification that is still needed.
* In multi-module `.cpp` files, file directives are the first lines of an
  anonymous namespace. Then `::name` reaches the module's own non-shared names
  exactly, because qualified lookup stops at the anonymous namespace that
  declares the name. `E0213` guarantees that nothing else in that namespace
  declares it, including library packs that put a directive at global scope
  and, through a build-time probe, headers included by Raw C++.
* A generated table of standard names (the union over GCC 11–15 and every
  supported standard) drives every decision. Raw C++ and library packs get
  explicit checks.

## Consequences

* Turning Textbook style on never breaks a project silently. It can require
  renames of namespace-scope types and namespaces named like standard ones, and
  `::` or `std::` insertions in Raw C++ whose meaning the directive would
  change (`E0218`). The preview lists both and applies them, after
  confirmation, in one undo step.
* Generated code mixes spellings where the user's names meet standard names
  (`::count`, `std::count` kept next to a user's `count`). Hovers explain each
  case.
* The work also fixed problems that exist without any directive:
  * calls to user functions captured by argument-dependent lookup, including
    hidden friends;
  * a global function hidden by a class member or a local;
  * members of dependent base classes, now written `this->m` or `D::m`;
  * overload sets split between shared and non-shared declarations, or
    between user code and library packs (`E0213`);
  * the anonymous-namespace layout inside user namespaces;
  * one dependency order for all declarations, so default member
    initialisers can use globals and functions;
  * placing Raw C++ declarations where both generated and raw code can use
    them.
* New maintenance: the standard-name table must be regenerated for each GCC
  release (a CI job per version checks it), and packs need generated name
  lists.
* The feature lands with the organisation blocks in M3, and its class and pack
  rules land in M4. The case programs of the studies become regression tests.
