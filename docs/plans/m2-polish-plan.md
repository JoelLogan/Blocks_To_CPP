# Blocks2Cpp: improvement plan after the owner's first M2 trial

> **Status: accepted (2026-10-10).** The owner accepted every recommendation of §6 and Appendix A (D1–D18), so the ADRs of §5 are *Accepted* when they are written (ADR-0012 to ADR-0014 in W0). Written 2026-10-10 from a review of the owner's first M2 trial; work proceeds wave by wave, and the spec and ADR revisions in §5 land with the work they describe.

This plan rests on the owner's 18 observations (U1–U18) and the reports of 14 investigation and audit agents. Everything was checked against HEAD `729a503` on branch `ccr-01f82d9b-v70u20`. This final revision also answers three reviews of the draft, on completeness, spec consistency and feasibility.

- Code citations are `path:line` at that commit.
- Blockly citations refer to the 12.5.1 sources (`core/…`).
- Anything marked *hypothesis* has not been confirmed.
- ADR numbers 0012–0016 are provisional and are assigned in the order the ADRs are written.

## 1. Summary

**What was found.** Every observation was traced to a cause. Most were reproduced, either in the real app (WebKitGTK under Xvfb) or with the real libraries (xterm.js 5.5.0, Blockly 12.5.1, the shared WASM core). Nearly all of them come from five root causes:

1. **Blockly and plugin defaults were adopted unchanged**, and they do not suit a Scratch-sized editor:
   - The flyout's width and scale follow the canvas zoom and the widest block (U6).
   - The default snap radius is 28 workspace units (U8).
   - The zoom "reset" control goes back to the start scale (U12).
   - `@blockly/continuous-toolbox` compares rounded scroll positions with fractional ones (U17).
   - The ≡ menu (z-index 50) competes with Blockly's toolbox (z-index 70) in one stacking context (U1).
2. **Two spec choices turned out wrong in use:**
   - ADR-0011 made every loose block an error that disables Run (U5).
   - The toolbox expands every variable into four blocks, and it lists the same block twice as presets (U3, U7).
3. **The listing point follows a selection that may be outside the program.**
   - A block that has just been dropped loose stays selected.
   - The scope query answers `[]` for it (`crates/b2c-lang/src/query.rs:168-181`).
   - So both the Variables category and the block's own variable menu go empty (U2).
   - Analyser errors on their own do not empty them.
4. **The chrome and the console have defects that no test looks at.**
   - Each run's prelude `ESC[?1049l` restores a cursor that was saved at row 0, so the next run overwrites the old output from the top (U4).
   - The fit add-on overflows its padded parent. At fractional display scaling, xterm 5.5 also ignores the wheel while output streams (U14).
   - The native `<select>` is drawn white on white in WebKitGTK (U15).
   - Copy confirmations never clear (U13).
   - Line coverage is 95%, but unit tests run in happy-dom without layout, and the E2E tests read byte transcripts rather than the screen.
5. **Each interaction does too much work (U11).**
   - In the 40-block guessing game, a click rebuilds the whole continuous flyout (130–165 ms).
   - At 1,000 blocks, an edit at the end of a long list re-renders every earlier statement (about 0.85 s).
   - Each edit loads and hashes the document twice in WASM.

Four observations are gaps in the spec and documentation:

- U9: *My Blocks* is unexplained jargon.
- U10: renaming a project was never specified.
- U16: themes follow the OS only.
- U18: the build steps are spread over 9 or more files.

**Beyond the owner's list:**

- **Two P0 bugs:**
  - Dropping a block on the toolbox while an insertion preview is shown connects the block and then deletes it, together with every block below the insertion point.
  - A negative number typed into a literal slot (`-1`) is rejected with B2C-E0310.
- **Several P1 items:**
  - The toolbox keeps a renamed variable's old name.
  - `b2c check` and the app report different problems.
  - *keep asking until valid* accepts `3.5` as 3.
  - The console is silent for screen readers.
  - The visual diff has never compared anything, because no baselines are committed.
- **Spec inconsistencies found by the reviews:**
  - The proposed latency goal reused the ID N10, which already means Reliability.
  - The format-version rules conflict with each other.
  - Upgrading blocks on load was not ordered against the limits and trust checks.
  - Several planned Blockly extensions go beyond what ADR-0002 allows.

**Approach.**

- **An M2 polish wave.** Insert it inside M2, before the usability sessions. Session participants must be first-time users, so they should not be spent on problems already known.
- **Must-have and if-time sets with a cut line (§3.1).**
  - The must-have set is 41 small, 13 medium and 1 large item. That is about 52 person-days at the low end of each estimate and about 84 at the midpoints.
  - Recommendation (D16): budget 8 weeks elapsed with two parallel streams. When the budget is spent, the if-time items move to M3 without further sign-off.
- **Decisions in batches.** Owner decisions are grouped by the wave they block (§6), so a pending answer holds up only the waves that need it.
- **Spec before code.**
  - Three new ADRs come in W0: parked blocks, format versions, and how Blockly may be extended.
  - Two more come before the work they govern: block upgrades on load, and the Web Worker in M3.
  - Every spec revision lands in the same PR as the code it describes.

| Wave | Content |
|---|---|
| W0 | Owner decisions (batched), ADR-0012/0013/0014, the anchor check, Dependabot, the issue tracker, spec corrections that need no code |
| W1 | Small verified fixes, the two P0 bugs, `BUILDING.md` |
| W2 | Performance guard: the long-task recorder, an A/B benchmark job, select and edit metrics |
| W3 | Layout, canvas controls, console behaviour, renaming the project |
| W4 | Parked blocks, the standard variable set; block migrations past the cut line |
| W5 | Placement: snapping; wrapping after a spike |
| W6 | The remaining documentation |
| W7 | M2 close-out: baselines, threat-model review, weekly run, usability sessions |

**Moved to M3:**

- the Web Worker preview;
- Blockly's render-management patch;
- Blockly 13 and xterm 6.1;
- lowering parked stacks for the scope query (U5-d);
- the type and slot foundations;
- whatever falls past the cut line.

Themes go on the future list (F-1).

## 2. Owner observations

| # | What is wrong (root cause) | Planned fix | Work items | Pri | When |
|---|---|---|---|---|---|
| U1 | `.main-menu-list` has z-index 50 (`apps/desktop/src/app/layout/MainMenu.css:20`) and loses to Blockly's `.blocklyToolbox` at z-index 70, because `.workspace` (`app.css:263`) creates no stacking context | `isolation: isolate` on `.workspace`; a documented z-index scale; Blockly pop-ups closed before a dialog opens | W1-1 | P1 | W1 |
| U2 | `listingPoint` (`editor/toolbox/scope.ts:112-125`) asks for the scope at a just-dropped loose block, and the core answers `[]` for blocks it never lowered (`b2c-lang/src/query.rs:168-181`). The block's own menu asks the core directly, with no fallback (`editor/services/symbols.ts:60-65`). Analyser errors do not empty the list | One scope resolver for the toolbox and the menus (W1-6); the core answers for parked and disabled blocks (W4-4); the Variables listing stops depending on the selection (W4-2, D2) | W1-6, W4-4, W4-2 | P1 | W1, W4 |
| U3 | A design choice: getter, set, change and update for each variable in scope, up to 400 blocks (`toolbox/contents.ts:196-237`). change and update are also offered for text variables, which always fail (E0302), and for true/false variables, which only warn (W0519) | A standard set: *Make a variable* with a type, `create`, one `set`, one `change`, one update block, menus filtered by scope and type, and one getter per variable (grouped, at most 50). Merging update into change only if time | W1-17, W1-19, W4-2; W4-1 and W4-5 if time | P1 | W1, W4 |
| U4 | `RUN_MODE_RESET` (`features/build-run/runController.ts:51`) sends `?1049l` on the normal screen. xterm then restores the cursor that the previous run's DECSTR saved at row 0 (`InputHandler.ts:2164-2172`), so run 2 overwrites run 1 | Prefix with `ESC 7`; scroll to the end when a run starts; Clear empties the console completely when idle; earlier output stays below a separator (D5) | W1-3, W3-7 | P1 | W1, W3 |
| U5 | ADR-0011: a loose block is E0604, which disables Run and makes `b2c check` exit 1, even when the block is disabled (`b2c-catalog/src/resolve.rs:231-259`) | ADR-0012: loose statements and values are *parked*. They get one W0505 warning, are never generated and never block Run. Catalog errors inside parked code still block. Lowering parked stacks so their own declarations work comes in M3 | W0-2, W4-3, W4-4; M3-4 | P1 | W4 |
| U6 | The flyout is as wide as its widest block times the canvas zoom: 644 px of an 894 px region at 1280×800, and the canvas is fully covered at 1024×700. Docks are fixed at 380/220 px (`EditorLayout.tsx:15-17`). The window is not fitted to the screen (`tauri.conf.json:14-24`). Nothing is remembered | A fixed flyout block scale and a user-set width with a splitter; docks sized as a share of the window; Tauri `preventOverflow`; first view clear of the flyout; remembered sizes if time | W3-1, W3-2, W3-3, W3-13; W3-4 if time | P1 | W3 |
| U7 | The catalog already has one `control.if` and one `control.while`. `catalog/toolbox.toml:102-105, 119-122` add a second preset entry for each | Delete the two entries; a new rule "one idea, one block, one toolbox entry", enforced by `b2c-catalog` | W1-8 | P1 | W1 |
| U8 | Default snap radius of 28 units (about 25 px at zoom 0.9, smaller when zoomed out). Blockly 12 cannot wrap statements that sit in a list (`connection_checker.ts:334`). Dropping on the toolbox during a preview deletes the stack (P0) | The P0 fix (W1-0a); a zoom-aware radius (W5-1); Scratch-style wrapping after a spike (W5-2, may move to M3) | W1-0a, W5-1, W5-3; W5-2 if time | P0/P1/P2 | W1, W5 |
| U9 | Scratch's term "My Blocks" is an unexplained heading inside Functions (`contents.ts:63, 326`). The category also offers calls to other modules' functions, which are error E0206 | Rename the heading (D11); a hint when no function exists; module headings only for two or more modules; only calls that can be used; no ⊖/⊕ on calls to known functions | W1-9 | P2 | W1 |
| U10 | Renaming was never specified. Every Empty project is "My Project" (`crates/b2c-app/templates/empty.b2c:10`). `project_save` does not update the recent list (`crates/b2c-app/src/projects.rs:457`) | Rename in place by clicking the title, plus *Rename project…* in ≡; a plain save updates the recent list | W3-6 | P2 | W3 |
| U11 | The flyout is rebuilt on clicks that change the scope (130–165 ms at 40 blocks); whole statement lists re-render (about 0.85 s per edit at 1,000 blocks); the whole canvas is read at drag start; each edit loads the document twice in WASM. The benchmarks measure none of this | Measurement and an enforceable gate first (W2); a flyout that does not depend on the selection (W4-2); a cheap pre-drag snapshot (W5-4); less churn per preview (P-2); a single `update()` export and a render spike if time (P-1, P-3); the Worker and the render patch in M3 | W2-1, W2-2, W4-2, W5-4, P-2; P-1, P-3 if time; M3-1, M3-2 | P1 | W2 onward, M3 |
| U12 | The crosshair is Blockly's `resetZoom`: back to startScale 0.9, then centre (`zoom_controls.ts:445-460`). The SVG controls have no names and no keyboard access. `zoom.wheel: false` (`EditorWorkspace.tsx:70-77`) | Named HTML canvas controls; *Show all blocks* keeps the zoom; Ctrl+wheel, pinch and keyboard zoom | W3-5 | P1 | W3 |
| U13 | The copy status is set and never cleared (`CodePanel.tsx:131-134`, `CopyCommand.tsx:23-28`, `LinkDialog.tsx:47-54`) | A shared `useTransientStatus`: success clears after 3 s and is announced again on every copy; failures stay | W1-4 | P2 | W1 |
| U14 | (a) Padding on the fitted parent (`panels/panels.css:320-325`) makes FitAddon propose one row too many, so the outer dock panel scrolls (verified). (b) At a fractional devicePixelRatio, xterm 5.5's viewport ignores the wheel while output streams (*likely*; the owner's scale factor is unknown) | Padding moved onto `.xterm`, no outer scrolling; an own wheel handler using `scrollLines`; coalesced refits; xterm 6.1 in M3 | W1-5, W3-8; M3-3 | P1 | W1, W3 |
| U15 | `.toolbar-select` is transparent with white text, but WebKitGTK draws the native control light in the light scheme (`app.css:209-223`) | `appearance: none`, an explicit background and chevron, and `.toolbar { color-scheme: dark }` | W1-2 | P1 | W1 |
| U16 | By design, themes follow the OS. The colour tokens are spread over `app.css`, `panels.css` and the Blockly themes | Future list F-1: an Appearance setting in M3 after a `[data-theme]` token refactor; High Contrast in M5 (F-2) | F-1 | P3 | M3 |
| U17 | `@blockly/continuous-toolbox` compares `Math.round(scroll/scale)` with fractional positions, and `if (this.scrollTarget)` treats 0 as "not animating" (`ContinuousFlyout.ts:146-162`) | Scroll to `position + 0.5` (verified: 0 mismatches in 192 clicks at 8 zoom levels); a check on heading texts; keep the category in view when Variables changes height (W4-2); report upstream | W1-7, W4-2 | P1 | W1, W4 |
| U18 | No document's only job is building. The first prerequisite in `apps/desktop/README.md` is at line 274 of 588. The documented way to run the app is the dev server | The anchor check first (W0); `BUILDING.md` early (W1); README split and consistency checks later | W0-5, W1-18, W6-3; W6-2, W6-4, W6-5 if time | P1 | W0, W1, W6 |

## 3. Work plan

### 3.1 Effort, budget and cut line

Effort scale:

- S: up to 1 day.
- M: 2–5 days.
- L: 1–2 weeks.
- XL: longer.

Person-days below count S as 0.5–1, M as 2–5 and L as 5–10. The midpoint estimate uses 0.75, 3.5 and 7.5.

| Wave | Must-have | M2 if time |
|---|---|---|
| W0 | W0-1 to W0-6 (6 S); W0-8 (M); W0-7 deferred (F-8) | none |
| W1 | W1-0a, W1-0b, W1-1 to W1-5, W1-7 to W1-17, W1-19 (19 S); W1-6, W1-18 (2 M) | none |
| W2 | W2-1 core (M); W2-2 (S) | the rest of W2-1 (M) |
| W3 | W3-2, W3-3, W3-7, W3-11, W3-12, W3-13 (6 S); W3-1, W3-5, W3-6, W3-8 (4 M) | W3-4, W3-9, W3-10 (3 M) |
| W4 | W4-6 (S); W4-3, W4-4 (2 M); W4-2 (L) | W4-5 (M); W4-1 (L) |
| W5 | W5-1, W5-4 (2 S); W5-3 (M) | the W5-2 spike (2 days), then W5-2 (L) |
| W6 | W6-3 (S) | W6-4, W6-5 (2 S); W6-2 (M) |
| Performance | P-2 (S) | P-3 (S); P-1 (M) |
| W7 | W7-2, W7-4, W7-5a (3 S); W7-3, W7-6 (2 M) | W7-7, H-1 (2 S); W7-1, W7-5b (2 M) |
| **Total** | **41 S, 13 M, 1 L: about 52 to 116 person-days, about 84 at midpoints** | **5 S, 9 M, 2 L plus a 2-day spike: about 33 to 72, about 52 at midpoints** |

**Budget (D16).** Recommended: 8 weeks elapsed with two parallel streams, about 80 person-days. That covers the must-have set at its midpoint estimate, with little room for the if-time items. One developer alone would need about 17 weeks for the must-have set. In that case the owner should narrow the must-have set further.

**Streams.**

- **Stream A: chrome, console, layout and docs.**
  - W1: W1-1 to W1-5, W1-7, W1-10, W1-11, W1-14, W1-15, W1-16, W1-18.
  - W3, W6, and W7-1/W7-2.
- **Stream B: language, core, editor model and placement.**
  - W1: W1-0a, W1-0b, W1-6, W1-8, W1-9, W1-12, W1-13, W1-17, W1-19.
  - W2-1, W4, W5, and the P items.

**Critical path.**

1. W0 batch C (D1–D3).
2. W4-3 and W4-4, in parallel.
3. W4-2, including its spikes.
4. Re-record the guessing-game exit E2E.
5. W7-2 baselines.
6. W7-6 sessions.

The core of W2-1 must land before W4-2, W5-1 and W3-11.

**Cut-line rule.** When the wave's budget is spent, if-time items move to M3 without further sign-off. A must-have item that runs late goes back to the owner with a choice: extend the wave or move the item.

### 3.2 Resolved conflicts between agents and reviews

- **Where the U2 fix lives.** The fix is done in two layers.
  - The core answers for parked blocks with the end-of-`main` scope (W4-4). This one answer is used by dropdowns and by paste re-binding.
  - Until then, an editor resolver serves both the toolbox and the field menus (W1-6).
  - Rejected: a TypeScript-only fallback as the final answer. It duplicates scope logic and leaves paste and the dropdowns disagreeing.
- **The Variables listing (U2, U3, U11, U17).** Recommended: option B of D2, a listing that does not depend on the selection. It removes three problems at once:
  - the empty list;
  - the flyout rebuild on every click that changes the scope, which is the largest measured lag in small programs;
  - the content shifts behind U17.

  W1-6 ships first because it is needed anyway for loose blocks.
- **Re-targeting after a drop (review).**
  - Only generic presets whose variable the user did not choose (*set*, *change*, update) are re-targeted.
  - It happens synchronously in `B2cBlockDragger.onDragEnd`, inside the drop's event group, so one Ctrl+Z removes the dropped block.
  - Getters are never re-targeted: they keep their reference and show E0203.
  - Rejected: re-targeting after the next preview. It would land after `Dragger.onDragEnd` has closed the event group (`eventUtils.setGroup(false)`) and could race the user's next edit.
  - Rejected: re-targeting getters, which would silently swap a variable the user picked by name.
  - A spike compares this design with doing no re-targeting at all (W4-2).
- **Declarations inside parked stacks (U5-d, review).** They are not lowered in M2. The limitation is written into ADR-0012, 03 §3.3, getting-started, and D1 as a sub-question. Lowering parked stacks in a sandbox is M3-4. Reasons:
  - it is an L item on the critical path;
  - the Scratch workflow (build a stack loose, then attach it) works as soon as the stack is attached.
- **W4 ordering (review).** W4-2 no longer depends on the migration work. It ships with today's `var.change` and `var.update`, each shown once. W4-1 (block upgrades) and W4-5 (merging update into change) sit past the cut line.
  - Rejected: the draft's order W4-1 → W4-3/4 → W4-2 → W4-5. W4-2 used a merged block that only W4-5 would create, and it put a file-format migration right before the first-user sessions.
- **Compatible catalog changes (review).** Adopted option (b): the catalog version counts. A newer catalog's unknown field is reported as E0602 (made by a newer catalog), not E0605, and the block is kept unchanged.
  - Rejected: a version bump for every new field, which would mark old files as changed on open.
- **Format versions (review).** ADR-0013 gives each new key its own version (2 = `usingNamespaceStd`, 3 = `lints`). The message for a newer file is E0108 and never names a version taken from the file.
- **Warning code for parked blocks.** Analyser lint `B2C-W0505`, not a catalog `W06xx`. Lint levels (M5) cover only W05xx/I05xx, so classrooms will be able to raise it to an error.
- **Console between runs.** The owner's complaint is explained by the U4 overwrite bug. Earlier output stays below a separator by default, and "Clear on each run" becomes an optional setting (D5). Always resetting (A04-09) was not chosen as the default, because it loses the earlier run.
- **Flyout parameters.** Block scale 0.75; default width min(natural, 320 px); range 160 px up to (region − 200 px); wider blocks are clipped. This was prototyped at runtime.
  - `reflowInternal_` is re-implemented rather than wrapped, so content changes do not trigger extra workspace resizes (review).
  - Rejected: a scale of 0.6, which makes blocks hard to read.
- **Drop on the toolbox (P0, review).**
  - A drag strategy computes "over a delete area" from the pointer event in `drag(newLoc, e)` and returns a search radius of −1 there.
  - `onDragEnd` runs one last `onDrag` before ending a deleting drop.
  - Nothing mutates the global `snapRadius`, which also drives bumping.
- **Window size (review).** Tauri's `preventOverflow` instead of custom monitor code.
- **Performance budgets (review).**
  - In M2, absolute budgets are reported, not gated.
  - Regressions are gated through an A/B job and an acceptance file.
  - The new latency goal N12 is the M5 target.
  - Rejected: gating guessed 50/100 ms budgets in M2. They would be red on day one, because flyout rebuilds measure 130–194 ms.
- **Wrapping rule (D13, review).** Recommended: option (a), Scratch's rule, which matches the owner's words. Option (b) ties with insertion under Zelos geometry.
- **U12 controls.** Replace them with HTML controls rather than patching the private `resetZoom`, which would stay inaccessible.
- **U17.** The `+0.5` slack, not "remember the clicked category": it is smaller and was verified.
- **U15.** Both `appearance: none` and `color-scheme: dark`. Each was verified on its own; together they look the same on WebKitGTK and WebView2.
- **README.** `BUILDING.md` is the single source, and it moves to W1 (review). The README keeps no commands.
- **Worker scope index (M3).** It does not duplicate the scope logic: the same `Analysis` produces it, and it is cross-checked against `symbols_in_scope` (ADR-0016).
- **Menu filtering (L6).** Moved to W1 (W1-19). It does not depend on option B.

### W0 — Decisions and groundwork

**W0-1 Owner decisions D1–D18, in batches** (§6). Effort S for preparing them, plus the owner's time.

- Batch A, before W1: D4, D7, D11, D14, D16, D17, D18.
- Batch B, before W3: D5, D8, D9, D10, D12.
- Batch C, before W4: D1, D2, D3.
- Batch D, before W5: D13.
- D6 and D15 block nothing in M2.

**W0-2 ADR-0012 "Loose blocks are parked".** U5.

- The text is in §5 item 1. Its status is *Accepted* (D1, decided 2026-10-10).
- ADR-0011 becomes *Partly superseded*, and the ADR README gains that status form (§5 item 2).
- Effort S.

**W0-3 Format versions (ADR-0013) and the newer-file message.** S5-2.

- **Cause:**
  - `crates/b2c-model/src/decode/project.rs:132-135, 161-169` accept only the M2 keys.
  - 05 §5.3 introduces the M3/M5 keys `options.usingNamespaceStd` and `project.lints` without a version step.
  - So an M3 file opened in M2 says "unknown key … Remove it or check its spelling" (E0110). Reproduced.
- **Change:**
  - Write ADR-0013 (§5 item 3).
  - Code in M2: an unknown key in a file whose `generator.app` is a newer SemVer than this app is `B2C-E0108` "made with a newer version of Blocks2Cpp".
  - The message never names a version taken from the file. Today's "needs ≥ X" text (`docs/reference/diagnostics/loader-and-catalog.md:129-137`) changes for this path.
  - The writer rule (use the lowest version that covers the file) needs no code until the first new key (M3).
- **Tests:**
  - b2c-model rule tests: an unknown key with `generator.app` 9.0.0 gives E0108, and its message contains no version number.
  - An equal or older `generator.app` still gives E0110.
  - Fixtures and malicious-project suite entries.
- Effort S.

**W0-4 ADR-0014 "Extending Blockly: allowed seams and patched dependencies".**

- ADR-0002 (`docs/adr/0002-block-editor-blockly.md:35-37`) allows "supported plugin APIs only". These items go beyond that:
  - W1-0a's drag strategy;
  - W3-1's protected flyout methods;
  - W5-2's replaced connection previewer;
  - P-3's patch.
- The ADR also adds a *Patched dependencies* row to 08 §8.9.
- The text is in §5 items 4 and 56. It is a prerequisite for those items. Its status is *Accepted* (D6, decided 2026-10-10).
- Effort S.

**W0-5 Anchor check over all Markdown.** U18-5.

- Add `node site/build.mjs --check-repo-markdown` to the docs job. It covers every `git ls-files '*.md'` file, excluding `node_modules`, `CHANGELOG.md` history, fuzz corpora and `.claude`.
- Today only spec and ADR anchors are checked (`site/build.mjs:245-320`), and lychee has no fragment checking.
- Also check the README anchors used in code strings (`apps/desktop/vite.config.ts:55`, `packages/b2c-core-wasm/scripts/build.mjs`).
- **Tests:** it passes on HEAD, and it fails when a heading in the desktop README is renamed in a scratch branch.
- Effort S. Prerequisite for W1-18 and W6.

**W0-6 Dependabot regrouping.** RD-3.

- **Cause:** `.github/dependabot.yml:11-36` puts every update of an ecosystem into one group. That blocks the tauri 2.12.1 patch (PR #1) behind breaking majors.
- **Change:**
  - One `minor-and-patch` group per ecosystem; majors stay ungrouped.
  - Ignore `version-update:semver-major` for blockly, `@blockly/*`, `@xterm/*`, typescript and vitest until a roadmap item schedules the migration.
  - Close PRs #1 and #2 and merge the tauri patch first.
  - Check GitHub's documentation on whether `ignore` also suppresses security pull requests (*hypothesis*). The 08 §8.9 wording (§5 item 56) covers either answer.
- **Tests:** the next Dependabot run opens one green minor/patch PR per ecosystem.
- Effort S.

**W0-7 Issue tracker.** RD-12.

- The repository has 0 issues.
- Add labels: owner-feedback, M2-polish, tech-debt, usability, perf, security-followup.
- Add a "Known issue" template with fields for evidence: commit, run, logs and hypothesis.
- Open one issue per U-item and per work item, and link them from 10 §10.1.
- *Status:* deferred by the owner on 2026-10-12 (future list F-8). The template `.github/ISSUE_TEMPLATE/known-issue.yml` exists; the labels and issues are not created, and this plan tracks the items instead (10 §10.1, 09 §9.1).
- Effort S.

**W0-8 Spec corrections that need no code.** These are the §5 revisions marked *correction*:

- 01 G3;
- 02 §2.3;
- 03 §3.7 "In M2", §3.7.7, §3.8/§3.9 notes;
- 05 §5.3 example, §5.9 trust text, §5.10;
- 06 §6.3, §6.8, §6.11, §6.12;
- 07 §7.3, §7.5, §7.6.1;
- 08 §8.13;
- 09 §9.1 and §9.3;
- 10 status-note corrections.

Tests: markdownlint, lychee and the anchor check. Effort M.

### W1 — Small verified fixes, the P0 bugs and BUILDING.md

**W1-0a Drop on the toolbox deletes stacks (P0).** Refs: R-DROP-DELETE, U8c. Data loss.

- **Cause:**
  - `Dragger.onDragEnd` decides `wouldDelete` from the pointer-up event (`core/dragging/dragger.ts:117`).
  - `endDrag` then applies the candidate left by the last `drag()` (`core/dragging/block_drag_strategy.ts:436-438`).
  - The root block is then disposed (`dragger.ts:131`).
  - `Gesture.handleUp` calls `onDragEnd` without a final `onDrag` (`core/gesture.ts:525-547`), so a release without a last pointer move keeps a stale candidate. That covers a fast flick, a touch, and a WebDriver action.
  - The always-open flyout is a delete area (`@blockly/continuous-toolbox src/ContinuousToolbox.ts:187-194`).
- **Change:**
  - **The strategy.** Add `B2cBlockDragStrategy extends Blockly.dragging.BlockDragStrategy`. `B2cBlockDragger.onDragStart` installs it with `BlockSvg.setDragStrategy` before calling `super`.
    - It goes on the non-shadow root: walk up `getParent()` while the block is a shadow, because a shadow's drag delegates to its parent (`editor/sync/drag.ts:56-60`).
  - **Spotting the delete area.** Override `drag(newLoc, e)`; `BlockSvg.drag` passes the event (`core/block_svg.ts:1828-1829`).
    - Before calling `super.drag`, set `overDeleteArea` from `workspace.getDragTarget(e)` having the DELETE_AREA capability.
    - An undefined `e` counts as false.
  - **The search radius.** `getSearchRadius()` returns `-1` while `overDeleteArea` is set.
    - A radius of 0 would still accept an exactly coincident connection (`core/connection_db.ts:253-256`).
    - Otherwise it returns Blockly's radius until W5-1 lands, and W5-1's radius after that.
  - **The final position.** In `B2cBlockDragger.onDragEnd`, when `wouldDeleteDraggable(e, root)` is true, call `this.onDrag(e, totalDelta)` before `super.onDragEnd`. The strategy then sees the final position, and `endDrag` has no candidate.
  - **Feedback.** `.blocklyDraggingDelete` fades the dragged block, and the flyout is tinted while the toolbox has `blocklyToolboxDelete` (`core/toolbox/toolbox.ts:603`).
  - **Upstream.** Report the bug to Blockly. *Status:* drafted in [`upstream/blockly-drop-on-delete-area-deletes-stack.md`](upstream/blockly-drop-on-delete-area-deletes-stack.md); the owner files it and records the link in `editor/sync/drag.ts` and the ADR-0014 seam list.
- **Tests** in `editor/sync/drag.test.ts`, with real pointer events. happy-dom reports every rect as 0, so stub `workspace.getDragTarget` to return a registered DELETE_AREA component when the pointer's x is below a threshold.
  - Move X near A.next over the canvas, then over the stub area: no insertion marker is shown. Drop: X is disposed and A→B→C is intact.
  - Move near A.next, then send `pointerup` over the delete area with no `pointermove` in between: same result.
  - A dragged shadow behaves the same.
  - Ctrl+Z restores X.
  - The E2E case is in W5-3.
- Effort S. Depends on W0-4.

**W1-0b Negative number literals (P0).** Ref: L1.

- **Cause:**
  - The number shadow saves `{"num":"-1"}` (`packages/blockly-ext/src/shadows/state.ts:109-110`).
  - `number_literal` (`crates/b2c-lang/src/lower/expr.rs:258`) has no sign handling, so the analyser says "`-1` is not a number".
- **Change:**
  - Add a `signed_number` helper and share it with `number_block`, including the `INT_MIN` case: `-2147483648` becomes `-2147483647 - 1`.
  - Add a specific message for octal-looking literals: "Write 10, not 010: in C++ a number that starts with 0 is octal."
- **Tests:**
  - `crates/b2c-lang/tests/types.rs`: `-5`, `-2147483648` and `+3` are accepted; `--1` and `-` still give E0310; `010` gives the new message.
  - A codegen snapshot.
  - A blockly-ext round trip: `-1` stays an editable literal after reload.
  - An E2E countdown that uses `change by -1`.
- Effort S.

**W1-1 Overlay stacking.** Refs: U1, C1, A04-04.

- **Change:**
  - `.workspace { isolation: isolate }`, verified at runtime.
  - A z-index scale written as a comment in `app.css`.
  - The dialog service gets an `onBeforeShow` hook, and the editor registers `Blockly.hideChaff()` on it. Today Blockly's DropDownDiv (z-index 1000) and WidgetDiv (z-index 99999) stay drawn over modal dialogs; this was verified.
- **Tests:**
  - A `styles.test.ts` rule.
  - E2E: `elementFromPoint` 10 px inside every ≡ item lands on that item, and 'Close project' clicked at its left edge works.
  - A dialog test: opening a dialog calls the hooks, and open Blockly pop-ups are hidden.
- Effort S.

**W1-2 Debug/Release drop-down.** Refs: U15, A04-17.

- **Change:**
  - `.toolbar-select { appearance: none; background-color: var(--b2c-toolbar-bg); color: var(--b2c-toolbar-text) }`.
  - A CSS or `data:` chevron; the CSP's `img-src` allows `data:`.
  - `.toolbar { color-scheme: dark }`.
  - Check the toolbar's height: with `appearance: none` the select's 28 px minimum makes the toolbar 40 px tall.
- **Tests:**
  - `styles.test.ts`: the appearance, the background token and a contrast of at least 4.5:1.
  - A Linux E2E crop of `[data-testid=toolbar-config]` with contrast of at least 4.5:1 in the light GTK theme.
- Effort S.

**W1-3 Console overwrite.** Refs: U4, S7-1.

- **Change:** `RUN_MODE_RESET = '\u001b7\u001b[?1049l\u001b[!p'`. Verified in four cases: plain output, scrolled output, a program that left the alternate screen on, and output without a final newline.
- **Tests:**
  - Vitest with a real xterm `Terminal` in `features/build-run/integration.test.tsx`: two runs, then Clear and a run, then a program that leaves `?1049h`, `?25l`, a scroll region and origin mode set.
  - Update the exact-string expectations in `feature.test.ts`, lines 416, 503, 521, 624, 627, 644 and 855.
  - An E2E hook `consoleScreen()` that reads the active buffer.
- Effort S.

**W1-4 Transient copy status.** Refs: U13, A04-11, S7-6.

- **Change:**
  - `panels/shared/useTransientStatus.ts`, used in CodePanel, CopyCommand and LinkDialog.
  - `.b2c-panel-status { white-space: nowrap }`, so the label no longer pushes the code down by about 25 px.
- **Tests:** fake-timer tests in the three suites:
  - the message is gone after 3 s;
  - a failure is still shown after 10 s;
  - a second copy clears the text and sets it again;
  - no state update happens after unmount.
- Effort S.

**W1-5 Console box overflow.** Refs: U14a, A04-10.

- **Change:**
  - Move the padding onto `.b2c-console-terminal .xterm`.
  - `overflow: hidden` on the host and on the console's dock panel.
- **Tests:**
  - A CSS audit.
  - E2E at three dock heights: `scrollHeight === clientHeight` on the dock panel, and the screen's bottom edge is inside the host.
- Effort S.

**W1-6 One scope resolver for the toolbox and the menus (interim U2).** Refs: U2-1, U2-a, A04-02.

- **Cause:**
  - `listingPoint` (`toolbox/scope.ts:112-125`) asks for the scope at the raw selected block.
  - The field menus call `core.symbolsInScope` directly (`editor/services/symbols.ts:60-65`).
  - A loose block, a block inside a disabled statement, and a block the latest analysis has not seen all get `[]`.
  - A selected declaration excludes its own variable (`crates/b2c-lang/src/query.rs:147-157`).
- **Change:** add `resolveScopeAnchor(workspace, block, source)` in `toolbox/scope.ts`.
  - **Who uses it:**
    - the toolbox's `variablesContents`, `loopsContents` and `takenVariableNames`;
    - the desktop symbol provider's `symbolsAt`.
    - `analysisSymbolSource` (`toolbox/plugin.ts:53-71`) delegates to that provider instead of duplicating it.
  - **Case 1.** The block's root is not `main` or a function (`!isInProgram`, `scope.ts:172-175`): use `endOfBody(main)`. With no `main`, the module's functions only.
  - **Case 2.** The block is in the program but its answer is empty:
    - walk to the previous statement in the same list (`{kind:'after'}`) or to the enclosing statement input;
    - the walk is bounded by `MAX_LIST_LENGTH`;
    - it never jumps to `main` from inside a function, because an empty function scope is legitimate;
    - this covers blocks inside a disabled statement, which answers for its own position, and blocks the latest preview has not seen.
  - **A selected statement** lists what is visible just after it, so a selected `create x` offers `x`.
  - **While a preview is pending,** the previous listing is kept.
  - **Later.** W4-4 removes case 1 once the core answers for parked blocks, and W4-2 stops the listing from following the selection. Case 2 stays for blocks that have not been analysed yet.
- **Tests:**
  - `scope.test.ts`: a loose head, a block nested in a loose C-block, a block inside a disabled `forever`, an unknown new block after P (its listing includes P's declaration), a fresh block inside a function (never gets `main`'s variables), and the bound.
  - A `services/symbols` test: a loose `var.get`'s menu lists `main`'s variables.
  - `contents.test.ts`.
  - E2E: drop `print` loose, and the getters are still listed. Drop `set` loose, and its dropdown lists `secret` and `guess`.
- Effort M.

**W1-7 Category selection jumps back.** Ref: U17.

- **Change:**
  - `B2cContinuousFlyout.scrollTo(position + 0.5)` (`toolbox/continuous.ts:153-156`), with a comment explaining the plugin's rounding.
  - `b2c-catalog` rejects a static entry label equal to a category name.
  - The dynamic headings live in `contents.ts`, so a TypeScript test in `contents.test.ts` checks that no heading or entry label equals a category name; the plugin finds categories by label text.
  - Report both plugin defects upstream. *Status:* drafted in [`upstream/continuous-toolbox-category-selection.md`](upstream/continuous-toolbox-category-selection.md); the owner files it and records the link in `toolbox/continuous.ts` and the ADR-0014 seam list.
- **Tests:**
  - A table test over 7 scales × fractional positions, with a relative error of 1e-12.
  - A happy-dom test that `super` receives p + 0.5.
  - E2E "keeps the chosen category at every zoom".
- Effort S.

**W1-8 One block, one toolbox entry.** Refs: U7, A04-13, L19.

- **Change:**
  - Delete `catalog/toolbox.toml:102-105` and `119-122`.
  - `b2c-catalog` rejects a second entry for the same block.
  - The help texts mention ⊕ and the while/until menu.
  - Regenerate `catalog.json`, `catalog.ts` and `docs/reference/blocks/{control,loops}.md`.
  - `apps/desktop/e2e/specs/exit/guessing-game.e2e.ts:183-189` drags `while` and chooses `until` from the menu.
  - Reword getting-started steps 5 and 7 (`docs/user-guide/getting-started.md:132, 143`) for the loop menu and ⊕.
  - Update `docs/manual-tests/m2-usability.md:148`.
  - No migration is needed: block types and versions are unchanged.
- **Tests:**
  - `contents.test.ts:206-217` rewritten.
  - `crates/b2c-catalog/tests/toolbox.rs:165-170`, plus a test that a duplicate entry is rejected.
  - `packages/catalog-gen/test/catalog.test.ts:93-99`.
  - The exit E2E on both systems.
- Effort S. Depends on D4.

**W1-9 Functions category wording.** Refs: U9, U9-2, A04-15, L20.

- **Change:**
  - Rename the heading (D11; recommended *Your functions*).
  - When no function exists, show the hint "Define a function above. A block that runs it appears here."
  - Show module headings only when there are two or more modules.
  - List only the active module's functions; calls to other modules are E0206 in M2.
  - Hide ⊖/⊕ on a call block whose function is known, so its argument count follows the definition.
  - Update `packages/catalog-gen/src/emit-markdown.ts:122` and the user guide.
- **Tests:**
  - `contents.test.ts`: the heading, the empty-state hint, the module headings for one and two modules, and the active-module filter.
  - A blockly-ext mutator test: a call to a known function shows no ⊖/⊕, and its arguments follow the definition.
- Effort S. Depends on D11.

**W1-10 Stale names in the toolbox after a rename.** Ref: A04-01.

- **Cause:**
  - `refreshSymbolNames` walks only the canvas (`packages/blockly-ext/src/shadows/state.ts:179-195`).
  - The flyout redraws only when its serialised contents change (`continuous.ts:84-98`), and those contents carry only `{ref}`.
- **Change:**
  - Also relabel the flyout's own workspace, then call `reflow()`.
  - Also run `refreshMutatorLabels` on the flyout after each preview.
- **Tests:** a `plugin.test.ts` case for a variable rename and one for a function rename.
- Effort S.

**W1-11 Small chrome fixes.**

- **Status link border (C2):** `.status-link { border: 0 }`.
- **Heading focus ring (C3):** show the ring only after keyboard use, through `data-input="keyboard"` on `<html>`. It is set on a Tab keydown and removed on pointerdown.
- **Unsaved marker (FA-11):** move `•` out of the ellipsised name, using a flex layout.
- **Tests:**
  - `styles.test.ts`: `.status-link` has border 0, and the marker is outside the ellipsised element.
  - A Vitest test that the attribute toggles on Tab and on pointerdown.
  - A Toolbar test with a 120-character name.
- Effort S.

**W1-12 *keep asking until valid* reads the whole line.** Ref: L4.

- **Cause:**
  - `crates/b2c-codegen/src/helpers.rs:149-151` accepts any valid prefix of the line.
  - `helpers.rs:75` `ask_line` uses `std::getline(std::cin >> std::ws, line)`, which skips blank lines silently.
- **Change:**
  - `ask<T>` reads one line, parses it with `std::istringstream`, and accepts it only when `(in >> std::ws).eof()`.
    - The prototype is verified: it rejects `3.5`, `12abc` and `1e400`, and accepts `"  7  "` and a last line without a newline.
  - `ask_line` strips a trailing `\r` and asks again on an empty line.
- **Tests:** stdin fixtures in `crates/b2c-codegen/tests/it/runtime.rs`, then the golden updates (`tests/golden`) and a manual guessing game with 50.5.

  | Input | Expected |
  |---|---|
  | `3.5\n4\n` | 4, plus one message |
  | `12abc\n7\n` | 7 |
  | `"  7  "` | 7 |
  | final value without a newline | accepted |
  | end of input | "Input ended", exit code 1 |
  | `\n\nBob\n` | the prompt is printed three times |

- Effort S.

**W1-13 `join` writes decimals like `print`.** Ref: L5.

- **Change:**
  - Add `b2c::to_text(double)`, which copies `std::cout`'s format (`out.copyfmt(std::cout)`).
  - Use it from `crates/b2c-codegen/src/lower/expr.rs:231, 252`, plus a `Helper::ToText` entry in `helpers.rs`.
- **Tests:**
  - A runtime test: a join of 2.5, 0.1 and 1e20 matches what `print` writes.
  - Helper tests and golden updates.
- Effort S.

**W1-14 The trash can keeps nothing.** Ref: FA-3.

- **Cause:**
  - `trashcan: true` (`EditorWorkspace.tsx:57`) keeps up to 32 deleted blocks as Blockly JSON across projects. Restoring them bypasses the validated clipboard.
  - The trash's flyout is the continuous flyout, which never closes by itself.
- **Change:** `maxTrashcanContents: 0` (D14). Undo restores deleted blocks.
- **Tests:**
  - Inject with `editorInjectOptions`, delete a block, and expect no stored contents.
  - An E2E drop on the trash can, then Ctrl+Z restores the block (in W5-3).
- Effort S. Depends on D14.

**W1-15 Blockly's default menu items.** Refs: FA-4, A04-07, RD-13.

- **Change:**
  - Unregister `blockInline`: it is never saved, so the change is lost on reload.
  - Replace `workspaceDelete` with an item that counts visible, deletable, non-shadow blocks. It confirms with `destructive: true` and *Cancel* as the default, and it deletes in one event group.
  - Keep *Clean up blocks* (D14) and amend 04 §4.13.
  - Restore every item on detach.
- **Tests:**
  - `registry.test.ts`: no `blockInline` after install, restored on uninstall.
  - The guessing game shows "Delete 11 blocks" (today: 22).
  - The confirm is destructive, and one Ctrl+Z restores all the blocks.
  - A snapshot of the offered menu item IDs for a block and for the canvas.
- Effort S. Depends on D14.

**W1-16 Run during the first compiler search waits.** Ref: S7-3.

- **Cause:** `apps/desktop/src/app/runGate.ts:51-53` returns `noToolchain` while discovery is still running, and `features/build-run/gate.ts:37-47` then opens the setup page.
- **Change:**
  - A new gate reason, `discovering`. Run stays enabled, and its tooltip says Run starts once a compiler is found.
  - The build waits (at most 30 s, 07 §7.2), and the build output says so.
- **Tests:**
  - runGate unit tests for `discovering` and for no toolchain.
  - A build-run feature test with fake IPC: Run before `toolchainsUpdated` starts a build and does not navigate.
- Effort S.

**W1-17 Labels name every field.** Refs: U3-4, L7. This was W4-6 in the draft; it moved here so the labels settle before W3-1 tunes the flyout width.

- **Change:**

  | Block | New friendly label |
  |---|---|
  | `var.declare` | "create %TYPE variable %NAME = %VALUE constant %CONST" |
  | `io.print` | "print %ITEM … %SEP new line %NEWLINE to %STREAM", with STREAM options *the console* / *the error stream* |
  | `text.char` | "character '%VALUE'" |

  - The char field accepts printable ASCII only.
  - A `b2c-catalog` lint: every field and every non-repeated value input appears in the label, and a checkbox has text next to it.
  - Label-only changes: no version bump.
- **Tests:**
  - catalog-gen label tests.
  - A blockly-ext layout test: "constant" comes before CONST, and "new line" and "to" appear in print.
  - The lint test.
  - A field test: `é` is refused.
- Effort S.

**W1-18 `BUILDING.md`, and entry points that use it.** Refs: U18-1, parts of U18-3, U18-8 and U18-9. This was W6-1 in the draft and moved earlier (review), because W2-2 and the owner's re-check need the release-build steps.

- **Content, at the repository root:**
  - a prerequisites table;
  - a Debian/Ubuntu block: CI's apt list (`desktop.yml:318-320`) plus binaryen, git and curl; rustup; Node 22.14+ with `corepack enable`; `cargo install wasm-bindgen-cli --version <Cargo.lock> --locked`; `pnpm install --frozen-lockfile`; the WASM build; `tauri build --no-bundle`; `./target/release/blocks2cpp-desktop`;
  - Fedora/Arch, marked untested;
  - Windows in PowerShell: MSVC, MSYS2 UCRT64 g++, `$env:B2C_SKIP_WASM_OPT`;
  - Run (the release build), Develop (the dev server, not for judging speed or looks), CLI, After you pull changes, Troubleshooting (including `WEBKIT_DISABLE_DMABUF_RENDERER=1`).
  - `pnpm desktop:build`, which builds every bundle type, appears only as an untested note: installers come in M6.
- **Entry points changed in the same PR:**
  - The README's *Get Blocks2Cpp* links to `BUILDING.md` and carries no commands (D7).
  - `getting-started.md:16-18` points to `BUILDING.md` § Run Blocks2Cpp.
  - The manual tests require a release build (`m2-usability.md:35-38`, `m2-accessibility.md:30-36`).
  - The `vite.config.ts:55` message points to `BUILDING.md`.
- **Tests:**
  - The W0-5 anchor check and markdownlint.
  - Follow it word for word in `debian:12` and `debian:13` containers, up to `tauri build --no-bundle` and an `xvfb-run` start (as in `desktop.yml:448`).
  - Follow the Windows section on a clean runner. A GitHub runner is a server edition whose PowerShell already runs scripts (RemoteSigned), so the check is also needed on a Windows client edition, whose default policy (Restricted) blocks Corepack's `pnpm.ps1`; `BUILDING.md` step 4 sets the policy for the user.
  - The owner confirms the commands without reading other files.
- Effort M. Depends on W0-5 and D7.

**W1-19 Variable menus leave out what cannot be changed.** Ref: L6.

- **Cause:**
  - `packages/blockly-ext/src/fields/symbol-ref.ts:48-49` filters out constants only.
  - The analyser rejects loop counters and read-only parameters (E0308, `crates/b2c-lang/src/lower/resolve.rs:117-136`), and text in change or update (E0302).
- **Change:**
  - `assignable` also excludes `loopVariable` symbols and `read_only` parameters.
  - A new `numeric` kind (int, double, char, error) for `var.change` and `var.update` (`blocks/catalog.ts:30-33`).
  - A reference that is already chosen stays shown (`symbol-ref.ts:140-142`).
- **Tests:**
  - Symbol-ref unit tests per kind.
  - Update `apps/desktop/e2e/specs/flows/editor.e2e.ts:70` to `['name', 'score']` and its comment at `:67-68`.
  - Add an assertion that `set`'s menu in the same fixture omits `i` and `limit`.
  - Run E2E on both systems in this PR.
- Effort S.

### W2 — Performance guard

**W2-1 Measurement and an enforceable gate.** Refs: U11-7, RD-8, U8d.

- **Why.** `tools/bench-compare.py` has three gaps:
  - it gates only nightly runs on the default branch;
  - it adds a run to the history only when nothing regressed (`:36-42`, `:495-498`);
  - it gates a metric only once it has 5 baselines (`:8`).

  So "W2 before W4-2" is not enforced at PR time. New metrics stay unguarded for at least 5 nightlies, and an intended slowdown would fail the nightly forever.
- **Must-have core (M):**
  - **Long-task recorder.** A MessageChannel heartbeat, as in `bench/page.ts`, in `e2e/support`, started by `launchApp`. Every flow reports its longest main-thread task; this is report-only in M2.
  - **A/B bench job.** Started by a PR label or `workflow_dispatch`. It builds base and head and runs the affected metrics alternately on one runner (at least 10 samples each), comparing medians at the 10% threshold.
  - **Acceptance file.** `apps/desktop/e2e/bench/accepted.json` holds entries of {metric, system, commit, reason}. `bench-compare.py` restarts that metric's history from the accepting run.
  - **Metrics:** `webview.select-1000.max-task`; `webview.edit-1000.p95` with the C++ panel open; guessing-game `select` and `edit` (report-only).
  - **Merge rule:** W4-2, W5-1 and W3-11 merge only with a green A/B run attached whose table compares their metric (a run that left their benchmark out says "Not conclusive"), or once their metrics have at least 5 baselines.
- **If time (M):**
  - `webview.drag-stack-1000.frame-p95`, `webview.drag-over-5000.frame-p95` and `declare-variable`.
  - User Timing marks around the pipeline phases, the toolbox refresh, the selection handler and the code-panel update. The bench reads them; in the app they are exposed only through the E2E hook.
  - Vitest work counters: flyout `show()` calls, store updates per run, scope queries per preview.
  - WASM core timing in PRs by instruction counts (iai-callgrind) or the gated criterion metric `native.pipeline.preview`, not by wall-clock comparison with `main`.
- **Tests:**
  - `tools/bench-compare.py --self-test` fixtures for accept and for A/B.
  - bench-unit Vitest for the new drivers.
  - Unit tests for the recorder's gap detection.
- Effort L, of which the core is M. The W2 gate depends on W2-1 only.
- *Status (2026-10-12):* the must-have core is merged; readings are local (debug backend, E2E frontend build, Linux under xvfb without a GPU).
  - *Long-task recorder:* `e2e/support/longtasks.ts`, started by `launchApp` and the flows' launcher. It uses a timer heartbeat instead of a MessageChannel one, which costs about 1.5 cores in WebKitGTK. It names the app's `b2c:` User Timing spans that overlap each task (P-2) and writes `long-tasks.md` into the E2E job summaries. Linux E2E run: 44 launches recorded, longest task per launch 147–792 ms (median 476 ms; a preview run with the C++ panel), and 3.0 s in the 10,000,000-line flood.
  - *A/B job:* `bench-ab-run.yml`, started by `bench-ab-pr.yml` (label `bench-ab`) or `bench-ab.yml` (by hand, the dispatched commit against one of its ancestors), `ab-rounds.sh` and `bench-compare.py ab`. It is linted (actionlint, zizmor) and has run on GitHub (runs 38331705145 and 38336846988); since W3 it runs each benchmark in its own Vitest call, skips with a notice a benchmark whose base build lacks a hook method it calls, leaves a failed benchmark out of the comparison, and fails only on a gated regression or a failure on the head. The first A/B, run locally, is P-2's (below; its selection readings were taken again after the review fixes).
  - *Acceptance file:* `accepted.json` (format version 2 since W3; the benchmark history is format 2 too). Each entry has a kind: `redefined` restarts a metric's nightly history only, `accepted` (an intended slowdown) also passes an A/B run. `edit-1000.p95`'s two entries are redefinitions.
  - *Metrics (re-measured 2026-10-13, after the review fixes below):* `select-1000.max-task` about 1.2 s (10 clicks, 353–1,414 ms). `edit-1000.p95` (gated; C++ panel shown, until the page settles) about 1.4 s. Guessing game: `select` about 140 ms (123–179 ms) and `edit` 31–49 ms (report-only). Per click, `b2c:select` takes about 0 ms and `b2c:toolbox-refresh`, the flyout's rebuild, about 550 ms at 1,000 blocks and about 120 ms on the guessing game (medians), so the rebuild W4-2 targets is most of a selection's cost. The longest task at 1,000 blocks is longer than the rebuild alone (a review probe found the press's own task at about 1.1 s for one target), so W4-2 should profile the press too. `console-200000.run` (gated, added by the review) about 0.75–0.8 s; its longest task about 110 ms (report-only).
  - *Review fixes (2026-10-13):* the first readings were wrong for selection. The benchmark's click holds the button 50 ms, and the app ignored the rebuild a selection asks for while the button was down (Blockly refreshes no toolbox during a gesture), so no guessing-game click and about half of those at 1,000 blocks rebuilt the flyout (the 15 ms above was a click without it), and users saw the previous scope after an ordinary click. The toolbox now rebuilds once the button is up, and a timed click fails unless the toolbox lists the clicked block's scope. `b2c:toolbox-refresh` wrapped only the scheduling of the rebuild (about 0 ms) and now covers the rebuild itself. On a pull request the A/B job measures the pull request merged into its base against that base, and runs every benchmark but cold start; its rounds start where the round before stopped, so they click all 8 scopes. The `console` benchmark gives W3-11's merge rule a metric. A panel hidden while another project is opened starts afresh.
  - *Merge rule:* in 09 §9.2.
  - *If time:* the User Timing spans and the Vitest counters for store updates and scope queries came with P-2. Not done: `drag-stack-1000` and `drag-over-5000`, `declare-variable`, the flyout `show()` counter, and instruction counts for the WASM core.

**W2-2 Profile on the owner's machine.** Refs: U11-9, U18-8.

- *Status:* protocol written ([`m2-performance-profile.md`](../manual-tests/m2-performance-profile.md)); the owner's run is pending. A release build is profiled as it is, through WebKitGTK's remote inspector (`WEBKIT_INSPECTOR_SERVER` and an `inspector://` window; checked with WebKitGTK 2.52.6 on a web view without developer tools), so no build with Tauri's `devtools` feature is added; 08 §8.8 records what the inspector exposes. The primary-monitor hypothesis is expected to hold: GTK 3.24's Wayland backend does not implement `gdk_display_get_primary_monitor`, which tao calls for Tauri's `center` and `preventOverflow`.
- The owner probably ran `pnpm desktop:dev`: a debug backend, React StrictMode, and no CSP.
- Re-measure U6 and U11 in a release build made with `BUILDING.md`.
- Profile the no-op clicks that took 130–210 ms (cause unknown).
- Collect: the WebKitGTK version, GPU, compositing mode, Wayland or X11, and the scale factor.
- Check whether `preventOverflow` sees a primary monitor in the owner's session. *Hypothesis:* GDK may report none on Wayland.
- **Output:** a profile record in `docs/manual-tests`. It informs the budgets but does not block W4 or W5.
- Effort S. Depends on W1-18.

### W3 — Layout, canvas, console behaviour, rename

**W3-1 Flyout scale and width.** Refs: U6a, R-FLYOUT, A04-05.

- *Status:* done. First, to fix the Windows E2E failures of Desktop run 38277029920 (35a9c6d): the 0.75 scale, the width min(natural, 320 px) narrowing down to 160 px to leave 200 px of canvas, `flyoutOpacity: 1`, the unit tests and an E2E check at the Windows runner's 1028×749. Then with W3-2 and W3-3: the user's width (160 px up to what leaves 200 px of canvas, at most 1200 px; kept while the window allows it, given back when it grows again), announced to the layout by a DOM event of the flyout (`app/layout/flyoutLayout.ts`, Blockly-free); only the toolbox's flyout announces itself, because Blockly's trash can builds its own flyout from the same registered class. The E2E checks at 800×560, 1024×700, 1028×749 and 1280×800 go through W3-3's window-size seam (`specs/flows/layout.e2e.ts`).
- **Change** in `B2cContinuousFlyout` (`toolbox/continuous.ts:146-173`):
  - `getFlyoutScale()` returns 0.75 (D9).
  - Re-implement `reflowInternal_` without calling `super`:
    - set `workspace_.scale = getFlyoutScale()`, because `VerticalFlyout.layout_` sets it straight to the canvas scale (`core/flyout_vertical.ts:227`);
    - compute the clamped user width (default min(natural, 320 px), range 160 px up to region − 200 px);
    - keep the last applied width in its own field, because `Flyout.show()` sets `width_ = 0` before reflowing (`core/flyout_base.ts:647-651`);
    - call `position()` and `targetWorkspace.resizeContents()` only when that remembered width changes. Since the tray-delete fix (dc02606..36a5e5d), a drop anywhere on the toolbox deletes after every rebuild (`specs/flows/tray-delete.e2e.ts`). *Status (perf fix, 2026-10-13, done):* the record of the drag targets is marked stale after every reflow, whatever the width, and by `position()` whenever the flyout moved or changed size (its private `place()` compares x, y, width and height); it is made again in the next animation frame, after Blockly's queued renders, or at the start of a drag that comes first (`sync/dragTargets.ts`), so a rebuild no longer forces a layout and a drag reads nothing when nothing moved. Tests: `dragTargets.test.ts`, `continuous.test.ts` ("costs a rebuild no layout"), `drag.test.ts`, and a drop after widening the flyout in `tray-delete.e2e.ts` (the C++ panel folded first in a small window, integration). A/B (integration, local, Linux under xvfb, debug backend, 2 rounds of 5 samples per side, base 43ad7bc, head the droptargets merge): `drag-5000.frame-p95` 57 → 56 ms (−1.8 %), `edit-1000.p95` 1,597.5 → 1,534.5 ms (−3.9 %), `preview-1000.p95` 119 → 105 ms (−11.8 %; this change does no work in the preview task, so read it as the run's noise), `select-1000.max-task` 1,110 → 1,129.5 ms (+1.8 %); the package's own A/B: `edit-1000.p95` −9.4 %, `select-1000.max-task` +6.0 % and −1.7 % in two runs. *Not done:* Blockly's own two records in every `Flyout.show` stay (they read the flyout while it is not displayed; removing them needs private members).
  - `flyoutOpacity: 1` in both Blockly themes (`packages/blockly-ext/src/theme/themes.ts:88-97`).
- **Tests:**
  - The scale stays 0.75 after `setScale(1.5)`.
  - The width never exceeds the user width.
  - The metrics' `left` equals the toolbox width plus the flyout width.
  - A work counter: two `show()` calls with an equal width call `targetWorkspace.resizeContents` 0 times.
  - E2E at 1024×700, using W3-3's window-size seam: a canvas strip of at least 200 px.
  - Re-run W1-7's table test.
- Effort M. Depends on W0-4, D9 and W1-17.

**W3-2 Toolbox splitter.** Ref: U6.

- *Status:* done. `Splitter` takes `side`, `className` and `style`, and Enter collapses only a panel that can be collapsed (`onCollapse` is optional). The *Resize the toolbox* splitter is laid over the canvas's left edge, at the flyout's right edge, placed from the flyout's announcement (`EditorLayout.css`); its range is the flyout's, 160 px to what leaves 200 px of canvas, and that is its `aria-valuemax`. *Deviation:* in the Tab order it comes after the canvas (and the canvas's own controls), not between the toolbox's blocks and the canvas as §5 item 35 had it: the only place between them in the document is inside Blockly's injection div, where Blockly's keyboard shortcuts would hear the splitter's arrow keys, Home and End first; it also keeps the canvas one Tab from the toolbox's blocks. Tests: `Splitter.test.tsx`; `EditorLayout.test.tsx` (keys, pointer, `aria-valuemax`, axe); `tabOrder.test.tsx`; `continuous.test.ts` (the width is kept when the canvas zooms); `layout.e2e.ts` (a pointer drag and End in the app).
- **Change:**
  - Generalise `Splitter.tsx`: a `side: before|after` prop, plus className and style props.
  - Position it at the toolbox width plus the flyout width inside `.workspace`.
  - It works with the pointer, the arrow keys and Home/End.
  - Add a stop for it in the accessibility checklist's Tab order.
- **Tests:**
  - Arrow, Home and End keys clamp to the range.
  - `aria-valuemax` equals the effective maximum.
  - axe passes.
  - Dragging changes the width, and zooming does not.
- Effort S. Depends on W3-1.

**W3-3 Window and dock defaults.** Refs: U6b, A04-08.

- *Status:* done. `preventOverflow` is on (config test `the_main_window_fits_the_screen` in `src-tauri/tests/consistency.rs`). Where Tauri finds no primary monitor, `window.rs` fits the window, once it is created, to `current_monitor()` or else the first of `available_monitors()`, never below 800×560, and logs at info level the monitor it used (its name, size and place), the bounds it fitted to, whether the work area was known, the scale and whether it resized; with a primary monitor it logs that Tauri fitted the window. The seam `B2C_E2E_WINDOW_SIZE` is read in `window.rs` with `e2e-hooks` only; a value that is not `WIDTHxHEIGHT` stops the app. `launchApp` and the flows' launcher take a `windowSize`, which `layout.e2e.ts` and `chrome.e2e.ts` use; the other flows and the benchmarks keep the app's own size (the integration ran the flows and exit specs at 800×560, 1024×700 and 1028×749 through the seam instead). The docks start as planned and follow the window until the user resizes them; the size shown is the chosen one kept within the limits (no dead zone), and the limit is the splitter's `aria-valuemax`. *Deviation:* the C++ dock's largest width is min(960, 65 % of the editor area, what leaves 200 px of canvas beside the narrowest flyout), not min(960, 65 %): with 65 % alone a drag could leave less than 200 px of canvas in any window narrower than about 1,330 px, and hiding the dock in the middle of a drag would surprise. So the flyout narrows first, then the C++ dock, which is hidden only when even its 200 px do not fit (a window below about 670 px) and shown again when it fits, unless the user showed or hid it meanwhile. Sizes with the 100 px category column of the Linux E2E app (C++ dock, flyout, canvas): 800×560: 280, 214, 200 px; 1024×700: 307, 320, 291 px; 1028×749: 308, 320, 294 px; 1280×800: 384, 320, 470 px. *Re-baseline (integration):* an A/B of every webview benchmark between 8d34abc (the old geometry) and the merged W3 (local, Linux under xvfb at 1280×1024, 10 samples per side) moved no gated metric by more than 6.6 %: cold start 2,029 → 2,029 ms, `preview-1000.p95` 114 → 112.5 ms, `edit-1000.p95` 1,532 → 1,627 ms (+6.2 %), `select-1000.max-task` 1,286 → 1,272 ms, `drag-5000.frame-p95` 58 → 58.5 ms, `console-200000.run` 843.5 → 899 ms (+6.6 %, W3-11). So `accepted.json` gets no entry. The Windows runner's geometry changed more (the C++ dock 308 px instead of 380): if its next nightly fails a gated metric on geometry, a `redefined` entry records it then. *Fix (W3 review):* GDK's Wayland backend has no work area and reports the whole monitor as one (`gdk_monitor_get_workarea`), so a 1366×768 screen got a 1280×768 window whose title bar and the panel pushed its bottom off the screen (reproduced under weston). Where the work area equals the monitor, the window is now fitted to the monitor less 64×100 CSS px (`fit_bounds`, unit-tested), and the log field `work_area_known` says which it was, so the owner's manual Wayland check is not misread.
- **Window:**
  - Set `"preventOverflow": true` on the main window in `apps/desktop/src-tauri/tauri.conf.json`.
  - Tauri 2.12 then clamps the window to the monitor's work area only when it would overflow (`tauri-runtime-wry-2.12.0 lib.rs:4463-4520`), and keeps 1280×800 wherever that fits.
  - No custom monitor code.
  - *Finding (W2-2):* GTK 3.24's Wayland backend has no primary monitor (`gdk_display_get_primary_monitor` returns none), and tao and `tauri-runtime-wry` use it for both `center` and `preventOverflow`, so under Wayland neither does anything. W3-3 therefore needs a fallback that fits the window to `current_monitor()` (or the first of `available_monitors()`) after it is created, or at least an info log line naming the monitor used. The owner's profile record confirms it.
- **E2E seam:**
  - An environment variable `B2C_E2E_WINDOW_SIZE=WxH`, read in `window.rs` only in builds with the E2E hooks.
  - Flows and benchmarks pin their size with it, and the size cases (800×560, 1024×700, 1280×800) set it per test.
  - WebDriver `setRect` may not resize the embedded webview (`canvas.visual.e2e.ts:83-100`).
- **Docks:**
  - The C++ dock starts at clamp(30% of the width, 280, 480) and the bottom dock at clamp(28% of the height, 120, 280).
  - The effective maximum is min(960, 65% of the container). A ResizeObserver measures the container, and the same value is used for `aria-valuemax`, which removes the dead zone.
  - Below 200 px of canvas, the flyout narrows first, then the C++ dock collapses.
- **Re-baseline:** the geometry changes the benchmarks' baselines.
  - Linux CI's `xvfb-run` screen is 1280×1024, and the Windows runner's window is about 1,030×750.
  - Record a bench re-baseline through `accepted.json` (W2-1).
- **Tests:**
  - A Rust config test that the main window has `preventOverflow`.
  - `EditorLayout.test.tsx` at 1366×768, 1024×700 and 800×560.
  - E2E at the Windows runner's size: the C++ dock is still shown.
- Effort S. Depends on D8.

**W3-4 Remember the layout (if time).** Refs: U6c, S5-5.

- *Status:* done. `settings.json` has a `ui.layout` section with the five keys, each absent until the user first changes it (the first-start rule of 04 §4.1 then applies), and each validated on its own: a value out of range or of the wrong type is reset with a notice naming it (`ui.layout.toolboxWidth`), a `ui` or `ui.layout` that is not an object is reset with a notice naming it, and unknown keys are kept (`b2c-store`). `settings_update` takes a partial `ui.layout` (`b2c-ipc`, `deny_unknown_fields` at every level, with the same ranges rechecked; `b2c-app` maps it both ways). The isolation allowlist, its samples and `ipc-types` are regenerated with the `b2c-ipc` generator, and `isolation-tests/validate.test.js` refuses out-of-range sizes, extra keys and wrong types. The nightly IPC-abuse E2E builds its cases from the generated samples (`e2e/specs/security/lib/abuse.ts`), so it covers `settings_update{ui}` without a change. In the app, the layout takes the remembered sizes from the settings once, before the first paint, and hands each change the user makes to a saver the settings feature registers (`app/layout/rememberedLayout.ts`), which writes the changes together 500 ms after the last one (`features/settings/layout.ts`), queued with the other updates; a failed write is only logged. The C++ dock hidden only because it did not fit is remembered as shown. W7-3's review covers `settings_update{ui}` and the wider allowlist. Tests: Rust (a round trip, the reset of each invalid value with unknown keys kept, refused updates, the IPC decode cases, the mapping, the snapshot); Vitest (`EditorLayout.test.tsx`: restored before the first paint or when the settings arrive later, only once, and each change handed on but not the hiding to fit; `settings.test.tsx`: writes debounced and merged, a failure only logged, the rest written at uninstall); E2E (`layout.e2e.ts`: after a restart the toolbox, the C++ dock and the hidden bottom dock are as they were left). *Fix (W3 review):* nothing is written when the window closes (the backend lets a window without unsaved changes close at once, and `main.tsx` never runs the features' uninstall), so a change made less than 500 ms before closing was lost (reproduced under xvfb: no `settings.json` at all). A settled change (a dock shown or hidden, the end of a splitter drag, Home or End) is now written at once, and only arrow-key steps wait the 500 ms (`Splitter`'s `onChange(value, settled)`, `rememberLayout(change, settled)`, `LayoutWriter.change`); tests in `Splitter.test.tsx`, `EditorLayout.test.tsx`, `settings.test.tsx` and an E2E case in `layout.e2e.ts` that closes the window right after hiding a dock and moving a splitter to its end.
- **Change:**
  - A `settings.json` `ui.layout` section: `toolboxWidth`, `rightDockWidth`, `bottomDockHeight`, `rightCollapsed`, `bottomCollapsed`.
  - Each value is validated, and an invalid one resets with a notice.
  - `settings_update` accepts a partial `ui`. Writes are debounced (500 ms).
  - Touches `b2c-store`, `b2c-ipc` (`deny_unknown_fields`), `ipc-types`, `b2c-app` and `features/settings`.
- **Isolation boundary:**
  - Regenerate the isolation allowlist and its samples, using the `b2c-ipc` generator checked by `src-tauri/tests/consistency.rs`.
  - Extend `isolation-tests/validate.test.js` with `ui.layout` cases: out-of-range numbers, extra keys and wrong types are refused.
  - Confirm that the nightly IPC-abuse E2E covers `settings_update{ui}`.
  - Add it to W7-3's review scope.
- **Tests:**
  - Rust: a settings round trip and the reset.
  - Vitest: the layout is restored and writes are debounced.
  - E2E: after a restart, the sizes are kept.
- Effort M. Depends on D8.

**W3-5 Canvas controls.** Refs: U12, A04-16, FA-2.

- *Status:* done. Blockly's SVG zoom controls are off and `zoom.wheel` is on, so `Ctrl` + wheel and a pinch zoom around the pointer and the plain wheel scrolls. `editor/view/zoom.ts` (`zoomStep`, `resetZoom` to 100 % around the centre, `showAllBlocks` keeping the scale; public Blockly API only) and `CanvasControls.tsx`: four named HTML buttons at the canvas's top right (*Reset zoom* shows the zoom now), portalled after Blockly's injection div so they come right after the canvas in the Tab order, placed as a Blockly `IPositionable` and again when the flyout's SVG resizes, never over the flyout. `Ctrl+=`, `Ctrl+-` and `Ctrl+0` zoom and announce the zoom. The bench page's drop report and `editor.e2e.ts` use the new controls; E2E `canvas-controls.e2e.ts`. Deviations: *Show all* centres or keeps the 24-unit margin per axis; zooming is instant (Blockly's canvas transition is internal). *Fix (W3 review):* the controls name `Ctrl+=`, `Ctrl+-` and `Ctrl+0` (hints, `aria-keyshortcuts`), but Blockly hears them only inside its injection `div`, which the controls are beside, so the keys did nothing while a control had the focus. The controls now handle them too (`zoomKeyOf`) and announce the zoom; `CanvasControls.test.tsx` and `canvas-controls.e2e.ts` press them on a focused control.
- **Change:**
  - `zoom: { controls: false, wheel: true }`.
  - A new `editor/view/zoom.ts`:
    - `zoomStep`;
    - `resetZoom`: around the centre, to D10's target;
    - `showAllBlocks`: keeps the scale; it centres the blocks when they fit, and otherwise puts the bounding box's top-left corner in view with a 24-unit margin.
  - `CanvasControls.tsx`: HTML buttons with names, hints and `data-testid`s, in the Tab order after the canvas.
  - Ctrl+= / Ctrl+- / Ctrl+0 on the canvas, with the new zoom announced.
  - Point `bench/page.ts:436` at the new controls.
- **Tests:**
  - `zoom.test.ts`: *Show all* keeps the scale; reset keeps the centre within 1 unit; Ctrl+wheel zooms and the plain wheel scrolls.
  - axe.
  - E2E: zoom in twice, press *Show all*, and the scale is unchanged.
- Effort M. Depends on D10.

**W3-6 Rename the project.** Refs: U10, S5-1, A04-12, FA-12.

- *Status:* done. `app/layout/ProjectTitle.tsx`: the name is a button that turns into a field (Enter or blur keeps, Escape cancels); `checkProjectName` trims, counts 1–100 code points, refuses line breaks and control characters and then asks `core.canonical()`. `EditorSession.updateProjectInfo` sets `project.name` as an unsaved change through the preview pipeline (the header comment follows; not undoable, D12). *Rename project…* in ≡. `project_save` now calls `touch_recent`, so a plain save renames the recent entry and moves it to the front. E2E `rename-project.e2e.ts` (rename, save, reopen from the recent list). *Fix (W3 review):* the window's capture-phase Save, Build and Run acted with the field still open, so the file got the old name. Save, Build and Run now finish a pending edit first (`registerPendingEdit` in `app/shortcuts.ts`, which the open field registers): a valid name is kept, an invalid one stops the key and says why. Tests in `shortcuts.test.ts`, `ProjectTitle.test.tsx` and a Ctrl+S-in-the-field step in `rename-project.e2e.ts`.
- **Change:**
  - `ProjectTitle` becomes a button that turns into an input. Enter or blur keeps the name, and Escape cancels.
  - The name is trimmed, has 1–100 characters on one line, and is checked with `core.canonical()`.
  - `EditorHandle.updateProjectInfo({name})` goes through the preview pipeline, so the project becomes unsaved and the header comment changes.
  - *Rename project…* is added to ≡.
  - Backend: `project_save` updates the recent entry's `projectName`.
- **Tests:**
  - Toolbar and MainMenu Vitest: Enter, Escape, blur, refused input, focus return, axe.
  - `crates/b2c-app/tests/requests.rs`: a plain save updates `projectName`.
  - E2E: rename, save, reopen from the recent list.
- Effort M. Depends on D12.

**W3-7 A run brings its output into view.** Refs: U4b, U4c.

- *Status:* done. `ConsoleHandle.scrollToBottom()` goes through the output scheduler and is issued by `RunSession` right after the run's prelude (the *New run* separator), so the scroll is ordered after it. An idle Clear empties the console completely (`clear()` plus `ESC[2K\r`, or `reset()` while a finished program left the alternate screen on); while a program runs, its prompt line stays. Integration tests with a real xterm; E2E in `console.e2e.ts`. *Fix (W3 review):* xterm.js's `clear()` does nothing on a screen that never scrolled with the cursor on its first row, so an idle Clear left the rows below and a running Clear did nothing. The idle branch now erases the whole screen after `clear()`, and the running branch erases the rows below the cursor's with relative moves in that case (`ConsolePanel.test.tsx`).
- **Change:**
  - `ConsoleHandle.scrollToBottom()`, applied in order through the scheduler when a run starts. It is a no-op in `DETACHED_CONSOLE` and `FakeConsole`.
  - When idle, Clear empties the screen completely: `clear()` plus `ESC[2K\r`. While a program runs, the prompt line stays.
- **Tests:** integration tests with a real xterm.
  - Scroll up 10 lines, start a run: `viewportY === baseY`.
  - Idle Clear leaves no lines; Clear while running keeps the cursor line.
- Effort S. Depends on D5.

**W3-8 Console wheel and refit.** Refs: U14b, U14c.

- *Status:* done; the cause is verified for WebKitGTK 2.52 at device scale 2 (GNOME's 125 % and 150 %): real wheel actions while output streams left the view at the end. `panels/console/wheel.ts` scrolls by lines with the remainder kept, only on the normal screen without mouse tracking or modifiers; `refit.ts` fits once per animation frame, rows at once and columns after 100 ms. The manual step at 125 % and 150 % is in `m2-usability.md`. WebView2 at fractional scales is the original 1.2 case, not re-checked.
- **Wheel:**
  - Use `attachCustomWheelEventHandler`, but take over only when the normal buffer is active, mouse tracking is `none`, and no modifier is held.
  - It converts pixel, line and page deltas into lines, keeps the remainder, calls `term.scrollLines`, and calls `preventDefault`.
  - Verified: 5 lines per notch at devicePixelRatio 1.2 while output streams.
- **Refit:**
  - Coalesce fits to one per animation frame.
  - Debounce column changes by 100 ms. Each reflow costs 11–38 ms at 10k lines of scrollback and about 100 ms at 100k.
- **Tests:**
  - WheelEvent unit tests: remainder accumulation, line and page modes, modifiers passed through, the alternate buffer passed through.
  - A fake-timer refit test.
  - A manual step at 125% and 150% scaling.
- Effort M.

**W3-9 Visible Undo and Redo (if time).** Ref: FA-5.

- *Status:* done. *Undo* and *Redo* in ≡, enabled from Blockly's undo and redo stacks and only on the editor screen; *Undo last change* on the sync-failure banner; a window-level `Ctrl+Z` / `Ctrl+Y` / `Ctrl+Shift+Z` (capture phase) outside text fields, dialogs, Blockly's own areas and a starting or running console. `features/project/limits.ts` names the limits a user can reach (`B2C-E0104`, `E0101`, `E0121`) in the banner and on Save instead of calling them a bug; the pipeline keeps the loader's codes with `syncFailed` (`AnalysisState.syncCodes`). *Fix (W3 review):* the window's `Ctrl+Z`/`Ctrl+Y` acted with the focus in the open ≡ menu, whose Undo and Redo states then went stale; they do nothing in a `role="menu"` now. The banner's *Undo last change* (and *Dismiss*) gives the focus to the workspace region when the banner goes away under it, instead of the body (`focusWorkspaceIfLost`; `analysis.test.tsx`).
- **Cause:**
  - Blockly binds its shortcuts on the injection div only (`core/inject.ts:75-79`).
  - `app/shortcuts.ts:52-70` handles only run, stop, build and save.
  - The sync-failure banner offers no action (`features/analysis/AnalysisNoticeBanner.tsx:20-21`).
  - Save calls the same state "a bug in Blocks2Cpp" (`features/project/lifecycle.ts:726-733`).
- **Change:**
  - *Undo* and *Redo* in ≡, through `EditorHandle.undo()/redo()`.
  - An *Undo last change* button on the banner.
  - A window-level Ctrl+Z / Ctrl+Y / Ctrl+Shift+Z, outside text fields, dialogs and a running console.
  - The banner's wording for the limits a user can reach (E0104, size).
- **Tests:**
  - MainMenu: the items are enabled from the undo and redo stack lengths.
  - The banner button.
  - `shortcuts.test.ts`: focus on the Problems grid undoes; focus in an input or a running console does not.
  - The lifecycle wording for E0104.
- Effort M.

**W3-10 Compiler discovery folder rules (if time).** Ref: S7-2.

- *Status:* done. Excluded folders have a kind (project, cache, current directory) and one rule: a project folder or the cache is excluded with its tree, except that a project folder that is the home folder or a root is excluded as that folder only; the current directory is always that folder only. The rule covers a rescan, the build-time check (`B2C-T1002`) and *Choose g++ manually…*. At startup the desktop app enters `<cache>/cwd` (empty, owner-only, never through a link); the CLI keeps the caller's folder. Skipped compilers (at most 64) are listed on the toolchain page with their reason; `toolchain_list` and `toolchainsUpdated` gain optional `skipped` and `rechecking`. *Fix (W3 review):* with the current directory in `<cache>/cwd`, GTK's save dialog (given no folder) opened there, so *Save project as* proposed the hidden cache folder (reproduced in the E2E app). The project dialogs now start in the project's own folder, else the documents or home folder (`project_start_folder`; the `Dialogs` trait passes Save as the project's folder); on Windows only the project's folder is given, as the common dialog never uses the current directory. 08 §8.5 and T10 now state the home/root exception and its residual risk.
- **Cause:**
  - Discovery excludes whole folder trees by subtree match (`crates/b2c-toolchain/src/discovery.rs:390-392`), and adds the current directory to the excluded list (`:59-64`).
  - Excluded folders: `crates/b2c-app/src/toolchains.rs:31-36`.
  - The app passes `cwd: None` (`apps/desktop/src-tauri/src/lib.rs:285`).
  - The build-time check uses the same subtree rule (`crates/b2c-build/src/toolchains.rs:740, 764`).
  - Started from `/`, the app finds no g++ (reproduced). Started from `$HOME`, it skips `~/.local/bin`.
- **Change:**
  - At startup the app changes its current directory to an empty owner-only folder, `<cache>/cwd/`. The CLI keeps the caller's.
  - The current directory is excluded as that folder only.
  - A project folder that is `$HOME` or a root is excluded as that folder only.
  - The same rule applies to a rescan, to the build-time check (T1002) and to *Choose g++ manually…*.
  - Skipped candidates are listed with their reason.
- **Tests:**
  - Discovery unit tests: excluding `/` or `$HOME` does not exclude `/usr/bin` or `$HOME/.local/bin`, while `/p/proj` still excludes `/p/proj/bin/g++`.
  - A registry test whose project folder is the fake home.
  - A src-tauri test that the current directory is inside the cache.
  - Toolchain-page Vitest for the skipped rows.
- Effort M.

**W3-11 Console output for screen readers.** Ref: FA-1.

- *Status:* done. `screenReaderMode` is on, paused for any single write over 16 KiB and resumed with a fresh accessibility tree 1 s after the last such write; Clear and the sink's reset start the tree afresh; a running program's Tab keydown is cancelled (the mode leaves Shift+Tab uncancelled). Not the `role="log"` mirror: measured in WebKitGTK 2.52, the mode always on cost +90 % unpaced and +5 % / +24 % at 4,000 / 8,000 lines per 16 ms; with the pause +1.4 % / +3 %. A/B of `webview.console-200000.run` at integration (local, Linux under xvfb, debug backend; 4 rounds, 20 samples per side): 862 → 897.5 ms (+4.1 %) for the console merge alone (8d34abc → 1b4a74c), and 851 → 932.5 ms (+9.6 %; a 2-round run: 843.5 → 899 ms, +6.6 %) for all of W3 against 8d34abc: green, under the 10 % gate, but close to it. Its longest task (report only) grows from about 120–134 ms to about 174 ms; likely the mode turned off at the flood's first big write or back on 1 s after the last (not profiled). The NVDA/Orca pass is W7-6. *Fix (W3 review):* xterm.js stops announcing after 20 lines until a key or a blur, and a run started with F5 from the console is neither, so the next run was silent. Each run now restarts the announcements where the console scrolls to the run (`scrollToBottom`), and the output scheduler no longer merges output queued after a scroll into the same write, so the run's first output is announced (`ConsolePanel.test.tsx`, `outputScheduler.test.ts`).
- **Cause:** `screenReaderMode` is off (`ConsolePanel.tsx:219-232`), and xterm marks `.xterm-rows` `aria-hidden`.
- **Change:**
  - `screenReaderMode: true`. The accessibility tree is reset together with the terminal.
  - Fallback if the cost is too high: a throttled `role="log"` mirror of the last 50 lines.
- **Tests:**
  - `.xterm-accessibility` exists and holds the text.
  - The existing tests for `consoleKeyAction` and the Tab trap pass with the mode on.
  - axe.
  - A flood E2E with the long-task recorder.
  - The NVDA/Orca pass (W7-6).
- Effort S. Depends on the W2-1 core, with an A/B run attached: its `console` benchmark (`webview.console-200000.run`, a program's 200,000 lines in the console) is the metric the mode can slow down.

**W3-12 Probe once more after a timeout.** Ref: RD-10.

- *Status:* done; the cause is confirmed: a compile round that timed out made the hello check fail, which gave `B2C-T1006`, and that record was reused while its fingerprint was current. A timed-out probe now reports `B2C-T1023` ("timed out … Rescan") instead of T1003/T1006, is probed once more with at least 30 s per test (`ProbeOptions::for_retry`) in scans, the build-time check, manual adds and the CLI's choice, and a timed-out record is never reused. `PROBE_FORMAT` is 2, so records of older builds are probed again once. The page says "Checking g++ again…" while the second probe runs (`toolchainsUpdated` at its start and end). Not changed: a second probe in `b2c-build` `compile.rs`'s own recheck (a compiler swapped between the registry's choice and the compile), which now reports T1023 with the Rescan advice. *Fix (W3 review):* a timeout in an optional check (a sanitizer's test program that hangs, hardening, static linking) made the whole compiler unusable (`B2C-T1023`); only the essential checks note a timeout now, and an optional one that times out only turns its capability off (`an_optional_check_that_times_out_leaves_the_compiler_usable`). The bump to `PROBE_FORMAT` 2 also made the first build after an upgrade report `B2C-T1009` ("probably updated") for an unchanged compiler: the note now depends on the fingerprint alone (`a_record_of_an_older_format_is_probed_again_without_a_note`).
- **Cause:** unknown.
  - *Hypothesis:* the first start of g++ on a cold Windows machine exceeds the 10 s probe timeout (`probe.rs:34`).
  - A failed probe is stored as unusable (`crates/b2c-build/src/toolchains.rs:1047-1066`).
- **Change:**
  - Read the problem codes from the next nightly occurrence first.
  - A probe that timed out is run once more with a 30 s timeout.
  - The setup page says "Checking g++ again…" while that runs.
- **Tests:** `crates/b2c-build/tests/toolchain_registry.rs`, with a fake prober.
  - A timeout followed by success ends up usable.
  - A probe that always times out gets exactly one retry.
- Effort S.

**W3-13 First view clear of the flyout.** Ref: U6c.

- *Status:* done; the cause was another one. Measured in the E2E app at 1280×800 after *Empty project*: the main block's left edge was exactly on the flyout's right edge (both at 420 px; the canvas at `translate(384, -36)`). The start page opens a project while the editor is hidden, and Blockly pads its scrollable area by half the view's size, which is then 0, so its only scroll position puts the blocks' top-left corner on the view's; a document without a saved view kept that view once the editor was shown. A width change of the flyout was not the cause: Blockly's scrollbars translate the canvas again when the flyout's width changes (checked in a unit experiment). Change: `settleView` shows a module without a saved view with its blocks centred along each axis where they fit with 24 units to spare, else with their top-left corner 24 units inside the view, once the editor has a size (the flyout then already fitted) and again after Blockly's first render. *Deviation:* no overlap check on width changes (none arises from them), and the centring applies to every document without a saved view, not only templates. Tests: three `session.test.ts` cases (each fails without the change) and `layout.e2e.ts` at four sizes.
- **Cause (*hypothesis*):** `settleView` (`editor/sync/session.ts:426-446`) restores the view before the flyout's first reflow widens it (`continuous.ts:33, 67-78`).
- **Change:**
  - After the flyout's first reflow, and whenever its width changes, check whether the top blocks' bounding box overlaps the flyout.
  - If it does, scroll right by the overlap plus a margin.
  - A template without a saved view calls `scrollCenter()` once the width is final.
- **Tests:** E2E after *Empty project*: the main block's left edge is at least 8 px right of the flyout's right edge, at 1280×800 and at 1024×700.
- Effort S.

### W4 — Block set, variables and parked blocks

**Order:**

1. W4-3 and W4-4 together.
2. W4-2.
3. W4-6.

Past the cut line: W4-1, then W4-5.

The guessing-game exit E2E is re-recorded once, at the end of the must-have part. Visual baselines are committed in W7-2, not here.

**W4-3 Parked blocks.** Refs: U5-a, ADR-0012, RR-2, A04-03, S5-7, L2, L9.

- *Status:* done (2026-10-13). `check_place` parks every top-level statement, reporter and predicate; `B2C-E0604` stays for the placements the editor cannot produce (a hat or definition inside a block, a statement in a value input, a value as a step, blocks stacked below a non-statement, reported once on that block), and every other catalog check still runs on parked code, disabled blocks included. `B2C-W0505` is reported once per enabled parked head with the stack counted; a reporter or predicate reads "This block gives a value, but it isn't plugged into another block, so it isn't used." Catalog messages name blocks by their label up to the first field or input (`short_name`, `block_noun`: *This "print" block …*, *This block …* when the label starts with one), also in the upgrade messages of W4-1 (integration); `E0601` and `E0620` keep the ID. `run_stages` always analyses a loaded document, so `b2c check` lists catalog and analyser problems together and exits 0 with only warnings; `crates/b2c-app/tests/problem_parity.rs` requires equal diagnostics from `run_frontend` and the preview over the examples, the security corpus and (integration) the block-upgrade golden files. Problems' block paths, and with them the canvas's keyboard announcements, name the program block *when program starts* (§5 item 31, third bullet; done at integration). Tests: `crates/b2c-lang/tests/parked.rs`, the rewritten E0604 tests, two property tests in `frontend.rs`, `cli.rs`, the parity test, `runGate.test.ts`, `roundTrip.test.ts`, E2E `flows/parked-blocks.e2e.ts`. *Deviations:* `E0620` still names the catalog ID (a bug report); the catalog's optional `short` key is left to M5; the parity test lives in `b2c-app`, where both front ends compile natively; the security corpus gains `parked-raw-cpp-block.b2c` (a parked Raw C++ block stays `E0601`). *Fix (W4a review):* an *ask* straight from the toolbox (no variable chosen; the editor saves it without `VAR`) was `B2C-E0606` wherever it lay, so a parked or disabled one held ▶ Run back. An absent `symbol_decl` or `symbol_ref` field is no catalog problem now; the analyser reports it (`E0430`) once the block is attached. And since the CLI analyses after catalog errors, one mistake was listed twice (`E0607` and `E0430` at one field, `E0602`/`E0603` and `E0430` on a block kept unchecked): `b2c_catalog::resolve_report` lists the blocks kept unchecked, and `b2c_lang::analyze_checked`, which both front ends call, leaves out an `E0430` at a field or input the catalog reported, at a function input of a block whose `extra` it reported, and on a block it kept unchecked. Tests: `crates/b2c-lang/tests/parked.rs` (every catalog block with a name or reference field, parked, disabled and attached), `crates/b2c-lang/tests/checked.rs`, `cli.rs`, `problem_parity.rs::each_problem_is_listed_once`, and an *ask* step in `parked-blocks.e2e.ts`.

- **Change:**
  - **Catalog resolution.** `resolve.rs check_place` (`:231-259`) reports nothing for a top-level statement, reporter or predicate.
    - Definitions are never parked.
    - E0604 stays for the placements the editor cannot produce.
    - Every catalog error except placement (E0601–E0603 and E0605–E0614) inside parked code remains an error. An absent name or reference field (nothing chosen yet) is not one: the analyser reports it as E0430 once the block is attached (W4a review).
  - **The W0505 lint.** `Lowerer::program` (`crates/b2c-lang/src/lower/mod.rs:231-256`) reports `B2C-W0505` once per enabled parked head, counting the stacked blocks.
    - A disabled head is silent.
    - Wording: "This block isn't attached to *when program starts* or a function, so it doesn't run."
  - **Block names in messages.** Catalog messages name blocks by their friendly label up to the first field, instead of IDs such as "io.print" (L9, U5-c).
  - **CLI parity.** `b2c check` and the build always run the analyser after a successful load (`crates/b2c-build/src/frontend.rs:161-172`), so the CLI and the app list the same problems (L2, U5-b).
  - **Reference docs:**
    - a W0505 entry in `docs/reference/diagnostics/analyser.md`; a test in `crates/b2c-lang/src/codes.rs:165-187` fails on an undocumented code;
    - E0604 narrowed in `loader-and-catalog.md:545-571`;
    - the exit code of `check` in `docs/reference/cli.md`.
- **Tests:**
  - **Rewritten:** the E0604 tests (`resolve.rs:883-1000`, `crates/b2c-catalog/tests/resolve.rs`).
  - **W0505:** once per head, the stack count, value wording, none for a disabled head.
  - **Properties:** parked stacks never change the generated C++, and parked blocks with an unknown type still give E0601 and stop the build.
  - **CLI and WASM:**
    - the CLI exits 0 with one warning, and `main.cpp` is byte-identical to the golden;
    - WASM gives `buildable = true`;
    - `runGate` allows Run.
  - **Parity:** a test over the examples and the security corpus that the CLI's codes equal the preview's.
  - **Updated expectations:** `roundTrip.test.ts:359` and the security-suite expectations (`tests/security/projects/README.md:64, 115`).
  - **E2E:** a loose `print`, then Run works.
- Effort M. Depends on D1 and W0-2.

**W4-4 Scope for parked and disabled blocks.** Refs: U2-b, U2-c, L3.

- *Status:* done (2026-10-13). The lowering records each module's end-of-`main` point (`body_and_end`, `ScopeIndex::record_main_end`), and `ScopeIndex::record_unlowered` gives every parked block, with its stack and everything nested in them, that point, iteratively and under `MAX_BLOCK_DEPTH`; without `main` they see the module's functions. A disabled statement, and a disabled value in a lowered input, record their whole subtree at their own point while lowering (`record_skipped`). `Analysis::scope_at` (`None`: no position), `scope_after` and `parked_scope` are the query; the WASM export `symbols_in_scope` and `CoreWasm.symbolsInScope` answer `null` for a block without a position. Paste on the canvas, or into or after a parked block, uses the parked scope. The desktop resolver keeps its fallback only for blocks the latest analysis has not seen (`symbolsAt` returns `null`); an empty answer is final; `symbolsAtConnection` is the scope W4-2's re-targeting asks for. The scope query answers for blocks an upgrade replaced, under the IDs they kept (integration test in `crates/b2c-core-wasm/tests/upgrade.rs`). Tests: `crates/b2c-lang/tests/scope.rs` (parked, no `main`, depth limit, disabled `if` and loops, skipped definitions), the linear-index unit test on 5,000-block disabled stacks, WASM scope and paste tests, `scope.test.ts`, `services.test.ts` and `toolbox/core.test.ts` (a loose `var.get` lists `main`'s variables from the core). *Deviations:* a definition the analyser skips (a disabled `main` or function) answers, with everything in it, with the module's functions; the core tells "no answer" (`null`) from "sees nothing" (`[]`), and an "after" answer hides a same-named outer variable, as `scope_after` does. *A/B (integration):* W4a (W4-1, W4-3, W4-4) against a559845, local, Linux under xvfb, debug backend, 2 rounds of 5 samples per side: `preview-1000.p95` 113.5 → 116 ms (+2.2 %), `edit-1000.p95` 1,756.5 → 1,626.5 ms (−7.4 %), `select-1000.max-task` 1,288 → 1,232.5 ms (−4.3 %); report-only, the guessing game's select 169.5 → 196.5 ms (+15.9 %) and, in a second run of that benchmark alone, 163 → 170 ms (+4.3 %), with the samples of both sides spread over 124–250 ms, and its edit 40 → 42 ms and 37 → 40 ms. No gated metric regressed, so `accepted.json` gets no entry. *Fix (W4a review):* the desktop fallback for a canvas block the analysis has not seen (and the listing and insertion point with nothing selected) used the topmost `main`, also a disabled one, while the core's parked scope is the end of the enabled `main` with the smallest block ID; `findMain` now picks that one (the topmost only when none is enabled). Without `main`, a new loose block's fallback lists the canvas's functions when the analysis has not seen the first definition yet (it listed nothing). The module docs of `session.rs` said the query answers `[]` after a failed preview; it answers `None`/`null`. Tests in `scope.test.ts`.

- **Change:**
  - While lowering, record the end-of-`main` point of each module.
  - Every parked block answers with that point: the head, its stack and the blocks nested in them, walked iteratively under `MAX_BLOCK_DEPTH`. With no `main`, it answers with the module's functions only.
  - Blocks nested in a disabled statement answer like that statement.
  - Paste re-binding into or after a parked block, or onto the canvas, uses the parked scope.
  - This removes the W1-6 fallback for parked and disabled blocks in `scope.ts` and `services/symbols.ts`. The fallback stays only for blocks the latest analysis has not seen.
- **Tests:**
  - `crates/b2c-lang/tests/scope.rs`, replacing `unknown_and_unattached_blocks_give_nothing` (`:416-428`).
  - A disabled `if` and a disabled loop.
  - The index stays linear on a 5,000-block disabled stack.
  - WASM paste re-binding.
  - The menu of a loose `var.get` lists `main`'s variables.
- Effort M.

**W4-2 Variables standard set.** Refs: U3-1 (both versions merged), U11-1, R-REBUILD, A04-14, U2b, FA-13.

- *Status:* done (2026-10-13), with D2 option B. *Variables* (`editor/toolbox/contents.ts`) offers *Make a variable* (a name, then a type: `int`, `double`, `bool`, `char` or `string`; the start value follows the type), the `create` block, one `set` and, while a whole-number, decimal or character variable exists, one `change` (W4-5's block with the operator menu; the catalog has no update block, so none shows), each preset to `main`'s first fitting variable by name (else the module's first), then a getter for every variable of the module, grouped by the function that declares it ("In when program starts", "In greet") and sorted by name, at most 50, then "…and N more". Nothing depends on the selection: the flyout is rebuilt only when `dynamicContentsKey` changes (the module's variables and functions, where the program starts, the modules), never during a drag, a field edit, an open menu or a category scroll (a scroll ends at its target, after standing still for 50 ms, or after 1 s), and it keeps the category in view (scroll anchor) and the selected category. A block from the toolbox is settled where it lands (`toolbox/landing.ts`, in `B2cBlockDragger.onDragEnd`'s event group): `create`, the counted loop and `define` get the first free default name there, and a generic `set` or `change` whose preset variable is not visible there takes the first fitting visible one (`symbolsAtConnection`), with `set`'s start value reshaped; getters keep their reference (`B2C-E0203`); one Undo removes the drop. A block added with the keyboard is settled the same way when it is put down (`keyboard/mover.ts`, integration). Blocks still in the flyout list every module variable in their menus, grouped (`setToolboxSymbols`); after review, a variable chosen there (or a type chosen in the flyout's `create`) reshapes the start value in the flyout too (`installStartValueReshaping` on the flyout's workspace), and the keyboard's ↓/↑ in the flyout also stop on *Make a variable*, the group headings and the notes (`keyboard/flyoutItems.ts`; a getter is announced with its group). Tests: `contents.test.ts`, `plugin.test.ts` (no `show()` on a selection change, one after a new variable; anchoring; the kept category), `continuous.refresh.test.ts`, `landing.test.ts`, `reshape.test.ts`, `makeVariable.test.tsx`, `drag.test.ts` (real pointer drags from the flyout with the core: one preview, one undo), `mover.test.ts` (keyboard adds), blockly-ext `fields.test.ts` and `services.test.ts`; E2E `exit/guessing-game.e2e.ts` re-recorded (Make a variable, `create` renamed, getters dragged into the comparison) and `flows/selection.e2e.ts` rewritten. *Spikes:* (a) re-targeting costs 0.30–0.44 ms per drop at the median at 1,000 blocks (Vitest with the real core; the preview is 48–56 ms and no second preview runs), so it stays and 04 §4.2 keeps its wording; (b) a whole rebuild after a new variable at 1,000 blocks takes 175–199 ms, and a section-level refresh would need Blockly's private flyout internals (ADR-0014), so it is not done and `declare-variable` stays ungated. *A/B (integration):* local, Linux under xvfb, debug backend, 2 rounds of 5 samples per side; base the droptargets and change-merge merges (80e5c20, with its Variables category listing *change* alone, since that catalog has no update block and the old per-variable listing failed on it), head the integrated wave: `select-1000.max-task` 1,059 → 1,009.5 ms (−4.7 %; bimodal: the clicks whose press is short fell from 327–901 ms to 70–218 ms, the others stay at 0.9–2.1 s on both sides, the press itself, for P-1 and P-3), `edit-1000.p95` 1,665 → 1,494.5 ms (−10.2 %), `preview-1000.p95` 111.5 → 122 ms (+9.4 %; the contents key costs 0.4–0.5 ms per analysis at 1,000 blocks, measured with the real core, so this is the run's noise, as the drag-target A/B's −11.8 % on the same metric); report-only, the guessing game's `select` 154 → 20.5 ms (−86.7 %) and `edit` 36 → 37 ms (+2.8 %). No gated metric regressed, so `accepted.json` gets no entry. *Deviations:* Program is dynamic in the editor only (W4-6); the selection benchmarks no longer check the toolbox's listing after a click (nothing is rebuilt per click; a target is only an ID); 04 §4.2's *Rebuilds* bullet names what is implemented; getting-started steps 2 and 3 follow the re-recorded exit flow; spike (a) was measured in Vitest, not in the webview. *Not done:* a Windows E2E run (CI).

- *Regression check (2026-10-15):* the GitHub A/B run 38491107208 (base 43ad7bc, head bee5c0a, 2 rounds of 5 samples per side) found `edit-1000.p95` 477.3 → 558.8 ms (+17.1 %, gated) on Windows and +8.5 % on Linux, with the preview's own spans unchanged. What one benchmark edit makes the page do was counted in the page (E2E builds of both, Linux under xvfb, 1,000 blocks, C++ panel shown; the same after 40 edits and with the panel folded), base → head: drag-target records 0 → 0, the flyout's `show`, `reflow` and `position` 0 → 0, `getBoundingClientRect` 55 → 54, Blockly's block renders 382 → 382, animation frames 14 → 14, the same DOM mutations (5,312 `transform`, 2,141 `d`, …), toolbox rebuilds 1–2 → 0, core results decoded 7 → 2; per drag, drag-target records 1 → 0. The one piece of work W4b added is the contents key: computed three times per edit (for the Blockly event of the added or deleted block, then twice in the preview's task, when the edit is stored and when its analysis is), it asked each variable's declaration for its root block, about n²/2 parent links for n variables (a statement's parent is the statement before it): 744 `getRootBlock` calls and 138,000 links per edit, 0.54 ms per key at 1,000 blocks and 9.2 ms at 5,000. `variableGroups` now follows each parent link at most once per listing (`rootFinder`): 0 such calls, 0.23 ms and 0.8 ms per key. Test: `contents.test.ts` counts the parent links of a key over 200 variables of `main` (20,300 before, at most 201 now). That is about 1.5 ms per edit at 1,000 blocks, far less than the Windows difference, and the rest is no work W4b added: the canvas has 744 more elements (W4-5's operator menu on the 248 *change* blocks: a border, an arrow image and a hidden image each; 25,102 → 25,846) and the flyout 6,102 fewer (7,017 → 915); hiding the borders and arrows of the canvas's 372 operator menus changed the forced layout of an edit by about 6 ms (paired, 15 pairs). Local A/B (Linux under xvfb, debug backend, 2 rounds of 5 samples per side): base 43ad7bc, head bee5c0a: `edit-1000.p95` 1,667.5 → 1,546 ms (−7.3 %), `preview-1000.p95` 134 → 110.5 ms (−17.5 %); base bee5c0a, head the fix: in 2 rounds of 5 samples, `edit-1000.p95` 2,188.5 → 2,669 ms (+22.0 %, flagged) and `preview-1000.p95` 107 → 108 ms (+0.9 %), where the fix's run of round 2 had samples of 2.6–5.5 s while other agents loaded the machine (its WebAssembly `b2c:preview` span 83.5 ms against 72–73 ms in round 1); in 4 rounds of 3 samples, `edit-1000.p95` 2,174 → 1,960 ms (−9.8 %) and `preview-1000.p95` 118 → 126.5 ms (+7.2 %): the same two builds moved by 32 points between two runs on this shared machine. The Windows result is most likely the runners' noise; a Windows A/B of the fix is to be run (CI). Seen meanwhile, on both sides: Blockly's `bumpNeighbours` after every render of `main` makes most of the 6,500 `getRootBlock` calls (1.2 million parent links) of each edit, for P-1 and P-3.

- **Category contents:**
  1. *Make a variable*, which asks for a name **and a type** (int, double, bool, char, string; int by default). The start value comes from `values.ts`.
  2. The `create` block.
  3. One `set [v ▾] to (start)`.
  4. One `change [n ▾] by (1)` and one `[n ▾] [+= ▾] (1)`, shown only when a number or character variable exists. They become the single merged `change` once W4-5 lands.
  5. One getter per variable of the module, grouped by the function that declares it ("In *when program starts*", "In *greet*") and sorted by name. At most 50, then a label "…and N more".
- **With D2 option B:**
  - The contents do not depend on the selection.
  - Remove the selection refresh (`plugin.ts:202-212, 254-263`). Refresh only when the key of declared variables and functions changes.
  - Presets refer to the first fitting variable of `main`, by name.
  - Free default names are chosen when the block lands on the canvas (`names.ts`), not baked into the flyout.
  - A block still in the toolbox lists every variable of the module in its menu, grouped like the category.
- **Re-targeting:**
  - It applies only to the generic presets (`set`, `change`, update), whose variable the user did not choose.
  - In `B2cBlockDragger.onDragEnd`:
    - record `Blockly.Events.getGroup()` before calling `super`;
    - after `super`, restore that group;
    - if the preset's variable is not visible at the new parent connection, set the first fitting visible variable (by name);
    - then close the group.
  - The scope at the new parent connection comes from W4-4's answer at the previous statement's *after* point or the enclosing input; the dropped block itself has not been analysed yet. One Ctrl+Z removes the dropped block.
  - Getters keep their reference. Out of scope, they show E0203, and their menu offers the variables that are visible.
- **Spikes (2 days each, inside W4-2):**
  - **Re-targeting:** compare synchronous re-targeting with no re-targeting under the 1,000-block benchmark. If (b) wins, change the 04 §4.2 wording to "keeps its variable…".
  - **Section-level refresh:** keep the static categories' flyout items and replace only the Variables and Functions items. The `declare-variable` budget is gated only if this lands.
- **Category anchoring:**
  - Before `flyout.show()`, record the category at the top of the view and the offset into it.
  - After `show()`, scroll to `getCategoryScrollPosition(category) + offset`.
  - Today Variables shifts later categories by 312 units per variable (`continuous.ts:84-96`).
- **Refresh rules:** no re-show while dragging, while a field editor is open, or while a category scroll animates. The selected category is kept.
- **Reshaping `set`:** `reshape.ts` swaps `set`'s value to the new type's start value when VAR changes and the value is still the old start value. This is one undo step.
- **Docs:** reword getting-started step 5's "click the loop to select it, open Variables" (`getting-started.md:134-137`).
- **Tests:**
  - **Contents and refresh:**
    - the Variables tests in `contents.test.ts` rewritten: one set/change/update each; change hidden without a number variable; grouping; the cap; equal items for an equal project;
    - flyout `show()` count is 0 on a selection change and 1 after a variable is added.
  - **Re-targeting:**
    - drag `greet`'s getter into `main`: it keeps its reference and shows E0203;
    - drag the `set` preset into `greet`: it is re-targeted in the same undo group, and a single undo removes it;
    - no second preview cycle runs (pipeline spy).
  - **Other behaviour:**
    - anchoring: scroll to Math, add a variable, and Math's label stays at the same y;
    - `makeVariable` with a type;
    - `reshape` cases.
  - **Measurement:** the W2-1 A/B run before and after.
- Effort L. Depends on D2, D3, the W2-1 core, W4-4 and W1-19.

**W4-6 Dynamic Program category.** Ref: FA-7.

- *Status:* done (2026-10-13). `programContents` (`editor/toolbox/contents.ts`) is the dynamic category `B2C_PROGRAM`: *when program starts* only while the project has none, otherwise the note "Your program starts at “when program starts”." (or "… in the module “X”." when another module has it), then the category's other entries (*stop program*). Where the program starts is part of the toolbox's contents key, so deleting `main` rebuilds the flyout. Tests: `contents.test.ts` with and without `main`, `plugin.test.ts` ("offers when program starts again once main is deleted", "says where the program starts when another module has it"); forcing the `main` entry fails two of them. *Deviation:* Program is dynamic in the editor only; `catalog/toolbox.toml` and its dynamic kinds (03 §3.11.1: Variables and Functions) are unchanged.

- **Change:**
  - *Program* offers *when program starts* only while the project has none.
  - Otherwise it shows a label saying where the program starts.
  - `contents.ts` plus `registerToolboxCategoryCallback`.
- **Tests:**
  - `contents.test.ts`, with and without `main`.
  - `plugin.test.ts`: deleting `main` refreshes the category.
- Effort S.

**W4-1 Block upgrades on load (if time).** Ref: U3-3.

- *Status:* done (2026-10-13); the shipped tables are empty until W4-5 adds `var.update`. ADR-0015 (amends ADR-0004). `b2c_catalog::upgrade_document` (`crates/b2c-catalog/src/upgrade.rs`) walks iteratively over the version chain and `BLOCK_REPLACEMENTS`/`RETIRED_BLOCKS`, keeps IDs, positions, comments, flags, stacks and nested blocks, checks each upgraded block (no added blocks, nesting or tokens, no changed declared ID, §5.6 limits) and then the whole document (security hash and summary equal, the document reloads), and leaves a block it cannot upgrade unchanged with `B2C-E0603`. It runs in `projects.rs load_file` (open, open recent, reload; after the §5.6 limits, before trust), in recovery restore, in the WASM `load` and `paste_prepare`, and in resolve (idempotent). A file or payload from a newer catalog reports unknown fields, inputs, `extra` keys, options and parts as `B2C-E0602` (D18 option b). `blocksUpgraded` in the open, reload and restore responses; the shell marks the project changed and says the original is kept as `.b2c.bak`. `b2c migrate` applies upgrades (`--in-place` keeps `.b2c.bak`; nothing is written on `E0603`), `fmt` never upgrades, and check, generate and build upgrade in memory. `upgrade_document` is in the resolve fuzz target and the mutation list (99 mutants, the one missed killed by a new test). *Deviations:* the mechanism is tested with test-only tables; `E0602` under a newer catalog also covers unknown inputs and statement parts; a violation found by the whole-document check undoes every upgrade of the document; the frontend shows no paste diagnostics yet, so a pasted newer block later shows `E0605` in the live preview; a restore shows no extra notice (it is already unsaved); `b2c-model` gains `is_newer_version`; test seam `Backend::start_with_block_upgrades`; upgrade messages name blocks by their label, a retired type as *a retired kind of block* (integration). *Fix (W4a review):* every save in the app replaced `generator.catalog` with the app's, so a file from a newer catalog, once saved in an older app, reported its newer blocks as unknown (`E0605`, "Remove it") instead of `E0602`. `withGenerator` keeps the newer of the two (`features/project/version.ts` mirrors `b2c_model::is_newer_version`; both are tested against `crates/b2c-model/tests/fixtures/semver-precedence.json`), and `b2c migrate` stamps `generator` the same way when it upgraded blocks (05 §5.3). A block kept unchecked (`E0602`, `E0603`) no longer gets the analyser's `E0430` as well (see W4-3's fix). Spec 05 §5.7 and ADR-0015 now describe the document-wide rollback, the package README the `load` result, and the CHANGELOG names `project_open_dialog`. Tests: `lifecycle.test.ts`, `version.test.ts`, the CLI's migrate tests, `problem_parity.rs`.

- **Cause:**
  - Migrations run only in `resolve` (`crates/b2c-catalog/src/resolve.rs:171`). Open, recovery and paste never apply them.
  - The editor turns every block with `v !== def.version` into a placeholder (`editor/sync/bdmToWorkspace.ts:209`).
  - `migrate.rs:63-67` forbids changing a block's type.
  - This contradicts 05 §5.7.
- **Change:** write ADR-0015 first (§5 item 5).
  - **The upgrade walk.** `b2c_catalog::upgrade_document` is an iterative walk.
    - It runs the version chain plus `BLOCK_REPLACEMENTS`, and keeps a `RETIRED_BLOCKS` list.
    - IDs, positions, comments, flags, stacks and nested blocks are kept.
    - A failure leaves the block unchanged and reports E0603.
  - **Where it runs:** `projects.rs load_bytes`, recovery restore, and the WASM `load` and `paste_prepare`. `resolve` keeps calling it; it is idempotent.
  - **Ordering.** It runs after the 05 §5.6 limits and before trust is evaluated.
    - It never adds blocks, nesting or tokens, and never changes an ID.
    - It never changes the security summary or security hash (08 §8.3.1).
    - The upgraded document is checked against §5.6 again.
  - **Newer catalogs.** `resolve` compares catalog versions. A document or clipboard payload that names a newer `generator.catalog` gets E0602, not E0605, for an unknown field, `extra` key or option.
  - **IPC.** `blocksUpgraded` in the open, reload and restore responses (02 §2.5.2). The shell marks the project changed and says the original is kept as `.b2c.bak` on save.
  - **CLI.**
    - `b2c migrate` applies block upgrades; `--in-place` keeps the `.b2c.bak`.
    - `b2c fmt` never upgrades.
    - check, generate and build upgrade in memory only.
- **Tests:**
  - Replacement and idempotence unit tests.
  - Golden files `crates/b2c-catalog/tests/migrations/*.b2c`.
  - b2c-app: open, save, `.bak`, `blocksUpgraded`.
  - WASM paste.
  - A file whose `catalog` is newer than the app's (1.2.0 opened by the 1.1.0 catalog) gives E0602.
  - The trust hash is unchanged by an upgrade.
  - `upgrade_document` added to the resolve fuzz target and to the mutation-testing list of 09 §9.2.
- Effort L.

**W4-5 Merge `var.update` into `var.change` (if time).** Ref: U3-2.

- *Status:* done (2026-10-13). `var.change` v1 has the menu `OP` (*by*, *down by*, *times*, *divided by*, *mod*; default `add`; a file without it still adds), lowered through `choice()` (an unknown value is `B2C-E0430` at the field); the catalog is 1.2.0. `var.update` v1 is retired and replaced on load by `var.change` v1 (`BLOCK_REPLACEMENTS`, `RETIRED_BLOCKS`; `VALUE` → `BY`, the same operator; an unknown operator, a non-text `OP` or a block that already has `BY` stays unchanged with `B2C-E0603`), in the app's open, reload and restore, the WASM `load` and `paste_prepare`, the CLI and resolve. The C++ is unchanged (`by` the literal 1 is still `++x`). Tests: `blocks.rs`, `types.rs`, `slots.rs`, `props.rs` and `checked.rs` in b2c-lang (the old update cases as `change_op`, the same SAST); `update_blocks_become_change_blocks` and `update_blocks_that_cannot_be_changed_are_kept`; the golden `tests/migrations/var_update.b2c`; a 1.2.0 file under the 1.1.0 catalog is `E0602` for `OP` (`upgrade.rs`); `update_blocks_open_as_change_blocks` (b2c-app), the WASM load and paste tests, and the CLI's `migrate` tests (every example byte for byte, the golden upgraded file); mutants of `upgrade.rs` 103 tested, 0 missed; `resolve` and `frontend` fuzzed 180 s and 120 s without a crash. After the merge the Variables category shows one *change* and no update block (integration). *Deviations:* no example used `var.update`, so none was migrated (the round trip is shown on every example); the C++ operators are named in *change*'s help text until the C++ label mode (M3) gives each option its own label; the CLI's `migrate` tests were added; the bench document's second change of a unit was `var.change` with `sub` (new parity digests); after review it is a plain `var.change` by `-1`, which every catalog since 1.0.0 reads, so an A/B base from before catalog 1.2.0 (such as 445a67e) measures the same program (`b2c check` of both documents is clean with 445a67e's CLI and the C++ is identical), and opening a benchmark document now fails on a build that shows any block of it as a placeholder (`openProjectFile`), so a base that cannot read the head's documents is "Not conclusive" instead of measured.

- **Change:**
  - `var.change` v1 gains `OP`: *by* / *down by* / *times* / *divided by* / *mod*, default `add`.
  - `var.update@1` is replaced by `var.change@1` (VALUE → BY) through W4-1, and is listed as retired.
  - `generator.catalog` goes to 1.2.0 (W1's label and help changes took it to 1.1.0).
  - The C++ output is identical.
  - **Also updated:**
    - `crates/b2c-catalog/src/toolbox.rs:86`;
    - `packages/blockly-ext/src/blocks/catalog.ts:32`;
    - `packages/catalog-gen/src/emit-markdown.ts:121` and `docs/reference/blocks/variables.md`;
    - `apps/desktop/e2e/specs/flows/lib/project.ts:136-140`;
    - `crates/b2c-core-wasm/benches/pipeline/document.rs:134`.
- **Tests:**
  - The same SAST as the old `var.update` cases.
  - A golden migration file.
  - A byte-identical round trip of `examples/primes.b2c`.
  - An old `var.update` file opens without a placeholder.
- Effort M. Depends on W4-1 and D3.

### W5 — Placement

**W5-1 Zoom-aware snap radius.** Ref: U8a.

- **Change:**
  - In W1-0a's strategy, `getSearchRadius()` = clamp(round(44 / scale), 48, 96) units.
  - `currentConnectionPreference` is 20 during drags. It is read from global config inside a private method (`block_drag_strategy.ts:344`), so it is saved and restored in a `finally` that also covers Escape-cancel and disposal of the gesture.
  - Bumping keeps Blockly's 28.
- **Tests** in `drag.test.ts`:
  - At scales 0.9 and 0.5, a drop 40 right and 20 down from an empty mouth lands in it.
  - A drop 120 away stays loose.
  - The config is back to 28/28/8 after a drop, a cancel and a throwing listener.
- Effort S. Depends on the W2-1 core, with an A/B run for the preview churn.
- *Status (2026-10-15):* done. `B2cBlockDragStrategy.getSearchRadius()` returns clamp(round(44 / scale), 48, 96) units (−1 over a delete area, as before), and `drag()` sets `currentConnectionPreference` for the length of each move and restores it in a `finally`, so nothing outside a move sees it: 20 while a statement is dragged, 0 while a value is. *Deviation:* with 20 for every drag the M2 exit E2E lost `guess = secret` to the getter, because a reporter's first slot lies 8 units from the slot that holds the reporter (03 §3.3 says a value goes to the nearest slot). Bumping keeps Blockly's 28. Tests in `drag.test.ts`: the radius at 7 zooms, the 40/20 drop into an empty mouth at 0.5, 0.9 and 1.5, drops 120 units (0.5, 0.9) and 100 units (0.25, the 96 cap) away staying loose, the preference between two mouths, a value into a comparison's first slot, and Blockly's 28/28/8 back after a drop, a cancel, a failing move and failing listeners; each fails against the unchanged code. The package's A/B of `drag-5000.frame-p95`: 56 → 55 ms; an ad-hoc preview-churn run (a print dragged along a 200-block list) showed 35 % fewer marker changes at 52 % zoom but a frame p95 of 77 → 88.5 ms (not gated). Integration A/B (local, Linux under xvfb, debug backend, base bee5c0a against the merged wave, 2 rounds of 5 samples per side in alternating runs): `drag-5000.frame-p95` 62 → 61.5 ms (−0.8 %). *Not gated (open):* `drag-5000` drags a definition across empty canvas, where nothing can connect, so it shows only that W5-1 does not slow such a drag; it never previews a place, and it is not the preview-churn A/B this item asked for. The cost of W5-1's previews has one reading, the package's churn run (+15 % frame p95 at 52 % zoom, above the A/B's 10 % line), which is open: a benchmark drag that previews (a statement dragged along a list at the start zoom and at about 0.5, §7's `drag-stack-1000`) and its A/B come with the radius decision below (M3). The bench README and 09 §9.2 say that `drag-5000` does not cover the previews. *Integration finding (open):* a statement preview moves the statements below it down, and with the larger radius the place the pointer passed stays in reach and nearer than the moved place below, so a block brought down a list keeps the place above until it is about a block's height further down. In WebKitGTK at zoom 0.5 an *if* brought down from 120 px above the gap between `main`'s second and third print still wrapped the second print at the gap (as its preview showed); with Blockly's 28 units the place above went out of reach and the gap was found again. The happy-dom sweep of W5-2 counts this as points where the approach decides: 585 / 259 / 250 of 625 for a plain *print* at 0.5 / 0.9 / 1.5, against 7–11 with Blockly's radius; the WebKitGTK sweep of the merged build, 37 / 13 / 13 of 49 for an *if* (0 before), still with no drop that differed from its preview. `placement-wrap.e2e.ts`'s fixed drops now come in from the right; whether to keep a smaller radius once a place is previewed (Blockly's `connectingSnapRadius`), or to measure against where the connections were before the preview, is for the usability sessions and M3.

**W5-2 Wrap stacks with C-blocks (if time; spike first).** Ref: U8b.

- **Spike (2 days, in both webviews):**
  1. Write W5-3 with the wrap cases skipped.
  2. Implement the checker, previewer and completion step.
  3. For drops at ±60 units around a gap, at zooms 0.5, 0.9 and 1.5, measure how often insert and wrap swap.
- **Spike outcomes:**
  - If the rule is predictable under D13's choice, continue (L).
  - If the spike fails, M2 ships W5-1 only. The wrapping sentence of 03 §3.3 gets "(M3)", and the wrapping half of the 10 §10.1 M2 bullet moves to M3.
- **Change:**
  - `B2cConnectionChecker.doDragChecks` (public, `connection_checker.ts:229`) allows a wrap pair: the empty first mouth of a statement C-block against a previous connection that holds a real block.
  - `B2cConnectionPreviewer` is registered as `CONNECTION_PREVIEWER`.
    - It re-implements marker creation, because `previewMarker` and `createInsertionMarker` are private (`insertion_marker_previewer.ts:113, 179`). Only `serializeBlockToInsertionMarker` is protected (`:162`).
    - It restores the exact stack on hide, and never lets the base `hideInsertionMarker` dispose a marker with user blocks inside.
  - `B2cBlockDragger.onDragEnd` reconnects the parent within the same event group.
- **Tests:**
  - Checker unit tests.
  - Real-pointer `drag.test.ts`:
    - preview, then move away: the canvas is unchanged;
    - drop: `[a, if{b, …}]`;
    - one undo restores, and redo re-wraps;
    - Escape during a wrap preview leaves the canvas byte-identical;
    - a nested mouth;
    - exact alignment and ±10 units.
  - A revert-drag property test.
- Effort L. Depends on D13 and W0-4.
- *Status:* done (2026-10-14). The spike succeeded with one change to how the place is found, and M2 ships wrapping (03 §3.3 "Placing blocks", 04 §4.7 and 10 §10.1 updated).
  - **The handle is the C-block's top, not its mouth** (a change of the rule the owner approved, which waits for the owner: D13's follow-up). D13 (a)'s result is kept: an empty statement C-block dropped on a statement takes its place, and the statement and everything below it in its list go into the first mouth; an empty C-block is never inserted between two statements. But the pair Blockly measures is the dragged C-block's previous connection and the connection above the statement (the next connection of the statement before it, or the statement input whose first statement it is), the pair an insertion would use, as Scratch 3 drops a C-block into a stack. The pair this item named (the first mouth against the statement's previous connection) is not predictable: its own preview puts the statements into the mouth, about 50 units from where they were, so the next pointer move is measured against the moved connection and finds another place or none. In the *mouth* rows below its preview flickers between a place and none in about 35 % of the drops at Blockly's radius (439–451 of 1,250, against 18–39 for the top), and the two approaches drop differently at 91–100 of 625 points (top: 9–12). At the top of a loose stack, which has no connection above it, Blockly's own pair (the mouth with the stack's first block) wraps the stack. At the end of a list, with nothing below to wrap, the C-block is inserted. Hats and definitions never take statements out of a list.
  - **Change.** `packages/blockly-ext/src/checker/wrap.ts` holds the rule: `wrapPlace(dragged, target)` (a pointer drag only: Blockly lists the dragged blocks' connections in `common.draggingConnections`, which a keyboard move does not fill), `completeWrap`, `isEmptyStatementCBlock` and `firstMouth`. `B2cConnectionChecker.doDragChecks` allows a wrap pair within the drag distance when the statements may go into the mouth (Blockly would insert the C-block, or refuse one without a next connection). `B2cConnectionPreviewer` (`previewer.ts`, registered as `b2c_previewer` and injected by `EditorWorkspace.tsx`) makes its own marker from the protected `serializeBlockToInsertionMarker`, puts it in the statements' place with the statements in its mouth, and on hiding puts them back where they were before it disposes the marker (a user block still attached is set loose, never disposed); every other preview is Blockly's own, all with events off. `B2cBlockDragger.onDragEnd` reads the wrap the preview shows (`previewedWrap`) before Blockly ends the drag (not when a drag target sends the block back), and after Blockly's drop moves the statements Blockly hung below the C-block into its mouth, in the drag's event group, before `settleDrop`: one Undo takes the whole drop back, and Redo wraps again.
  - **Escape.** Blockly 12.5.1 has no Escape-cancel for a pointer drag: Escape only closes pop-ups, and `Gesture.cancel` drops at the pointer. Escape during a wrap preview therefore changes nothing, and a drop back where the drag started leaves the document byte for byte as it was (tested); a drop that a drag target sends back restores the canvas exactly, also for an empty C-block taken out of a list and brought back over its old place (tested).
  - **Memory.** Blockly 12.5.1's render queue keeps for good every block that is unplugged and disposed before it is drawn (`render_management.ts`, `rootBlocks`): every deleted block of a stack and every statement insertion marker its previewer hides. The wrap previewer draws its marker before disposing it, so its markers are freed (under `--expose-gc`, 20 of 20 kept without that, 0 with it); Blockly's own markers and deleted blocks leak until Blockly fixes it. Upstream report drafted: [blockly-render-queue-retains-disposed-blocks.md](upstream/blockly-render-queue-retains-disposed-blocks.md).
  - **Tests.** `checker/wrap.test.ts` (blockly-ext, 19): the rule, the checker's pair (allowed where Blockly refuses, the drag distance, a mouth that refuses the statements), the previewer (registration, exact restore with no non-UI event, moving between wraps and Blockly's preview, a stray block set loose, its marker drawn before it is disposed) and `completeWrap`. `editor/sync/dragWrap.test.ts` (desktop, 18, pointer events through Blockly's gestures on the guessing game): preview then move away leaves the canvas as it was; the drop gives `[b002, b003, if{b004, b010}]`; one Undo (one event group, settled once) and Redo; Escape; a nested mouth; the first statement of a list; exact alignment and 10 units off in each direction; the end of a list; a loose stack's top; a delete area after a wrap preview; the editor injects the checker and previewer; a keyboard move is inserted; a revert-drag property test (25 seeded runs) and an empty C-block taken out of a list and sent back. `editor/sync/wrapSweep.test.ts` checks the shipped rule on a 20-unit grid at the three zooms on every run (no mismatch, no place flicker, never an insertion), with both Blockly's radius and W5-1's since the review fix below. `e2e/specs/flows/placement-wrap.e2e.ts` (real pointer input): an *if* from the toolbox previews and wraps `[a, if{b, c}]`, moving away hides the preview, Ctrl+Z restores exactly and Ctrl+Y wraps again; a loop's body is wrapped from its top; at the zooms 0.5, 0.9 and 1.5, at the Windows runner's window size (1028 × 749), an *if* from the toolbox dropped exactly and 10 units off in each direction wraps the statement below the gap, as its preview showed. Each behaviour change was checked against its test by taking it out: the checker's pair, the previewer's draw before dispose, the dragger's completion and its return-to-start guard, and the editor's previewer each make a test fail.
  - **Measured in happy-dom** (`editor/sync/wrapSweep.test.ts` with `B2C_WRAP_SWEEP`; Blockly's gestures and the Zelos geometry, with text widths of 0): an empty *if* dragged onto `main`'s five prints with its top on a 5-unit grid over ±60 units around the gap between the second and third (625 points), each point twice (from 150 units above and from below, in 10 moves, then five 1-pixel shakes), 1,250 drops per row. *Mismatch*: the drop did something else than the preview at the release. *Place flicker*: while the pointer shook, the preview went back and forth between two places; *edge flicker*: between a place and none, at the edge of the radius. *Lag*: the preview changed once at the drop point (a preview moves the statements below it, so the next place down is found a move late, as for Blockly's insertion). *Swaps*: points where the two approaches dropped differently (Blockly prefers the place it already previews); *insert/wrap swaps*: one approach inserted the *if* and the other wrapped. Rules: **top** (shipped), **mouth** (the pair this item named), **either** (D13 (b) without a tie-break: the mouth pair and Blockly's insertion), **print** (the baseline: a *print*, which Blockly inserts). Radii: Blockly's (28 units, a preference of 8 for the place already previewed) and W5-1's (44 px, 48–96 units, a preference of 20).

  | Rule | Radius | Zoom | Drops | Mismatches | Place flickers | Edge flickers | Lags | Swaps | Insert/wrap swaps |
  |---|---|---|---|---|---|---|---|---|---|
  | top | Blockly (28, 8) | 0.5 | 1250 | 0 | 0 | 39 | 50 | 9 of 625 | 0 |
  | top | Blockly (28, 8) | 0.9 | 1250 | 0 | 0 | 25 | 50 | 11 of 625 | 0 |
  | top | Blockly (28, 8) | 1.5 | 1250 | 0 | 0 | 18 | 46 | 12 of 625 | 0 |
  | top | W5-1 (44 px, 48–96, 20) | 0.5 | 1250 | 0 | 0 | 0 | 54 | 578 of 625 | 0 |
  | top | W5-1 (44 px, 48–96, 20) | 0.9 | 1250 | 0 | 0 | 12 | 104 | 249 of 625 | 0 |
  | top | W5-1 (44 px, 48–96, 20) | 1.5 | 1250 | 0 | 0 | 4 | 106 | 239 of 625 | 0 |
  | mouth | Blockly (28, 8) | 0.5 | 1250 | 0 | 8 | 451 | 4 | 91 of 625 | 0 |
  | mouth | Blockly (28, 8) | 0.9 | 1250 | 0 | 4 | 443 | 2 | 100 of 625 | 0 |
  | mouth | Blockly (28, 8) | 1.5 | 1250 | 0 | 4 | 439 | 2 | 91 of 625 | 0 |
  | mouth | W5-1 (44 px, 48–96, 20) | 0.5 | 1250 | 0 | 0 | 0 | 41 | 469 of 625 | 0 |
  | mouth | W5-1 (44 px, 48–96, 20) | 0.9 | 1250 | 0 | 11 | 314 | 36 | 225 of 625 | 0 |
  | mouth | W5-1 (44 px, 48–96, 20) | 1.5 | 1250 | 0 | 7 | 300 | 36 | 219 of 625 | 0 |
  | either | Blockly (28, 8) | 0.5 | 1250 | 0 | 12 | 112 | 81 | 26 of 625 | 1 |
  | either | Blockly (28, 8) | 0.9 | 1250 | 0 | 8 | 98 | 76 | 30 of 625 | 0 |
  | either | Blockly (28, 8) | 1.5 | 1250 | 0 | 7 | 89 | 76 | 33 of 625 | 1 |
  | either | W5-1 (44 px, 48–96, 20) | 0.5 | 1250 | 0 | 0 | 0 | 165 | 614 of 625 | 390 |
  | either | W5-1 (44 px, 48–96, 20) | 0.9 | 1250 | 0 | 7 | 97 | 103 | 342 of 625 | 197 |
  | either | W5-1 (44 px, 48–96, 20) | 1.5 | 1250 | 0 | 7 | 81 | 102 | 336 of 625 | 192 |
  | print | Blockly (28, 8) | 0.5 | 1250 | 0 | 0 | 35 | 54 | 7 of 625 | 0 |
  | print | Blockly (28, 8) | 0.9 | 1250 | 0 | 0 | 31 | 46 | 11 of 625 | 0 |
  | print | Blockly (28, 8) | 1.5 | 1250 | 0 | 0 | 20 | 49 | 11 of 625 | 0 |
  | print | W5-1 (44 px, 48–96, 20) | 0.5 | 1250 | 0 | 0 | 0 | 48 | 585 of 625 | 0 |
  | print | W5-1 (44 px, 48–96, 20) | 0.9 | 1250 | 0 | 0 | 8 | 85 | 259 of 625 | 0 |
  | print | W5-1 (44 px, 48–96, 20) | 1.5 | 1250 | 0 | 0 | 6 | 91 | 250 of 625 | 0 |

  - **Reading.** The shipped rule never swaps insert and wrap (no insertion between statements is offered), and behaves as Blockly's own insertion does (*top* against *print*: no mismatch or place flicker, edge flickers 18–39 against 20–35, lags 46–50 against 46–54, swaps 9–12 against 7–11 of 625). D13 (b) without a tie-break swaps insert and wrap at 0–1 points with Blockly's radius but at 192–390 with W5-1's. W5-1's radius lets the way the pointer came decide most drops for every rule, Blockly's own insertion included (*print*: 585 / 259 / 250 of 625 at 0.5 / 0.9 / 1.5; *top*: 578 / 249 / 239): with several places 48 units apart in reach, the preference of 20 keeps the place the pointer passed. That is a property of W5-1's radius and preference (for W5-1 and its A/B run to weigh), not of wrapping.
  - **Measured in WebKitGTK** (`placement-wrap.e2e.ts` with `B2C_E2E_WRAP_SWEEP=1`; the E2E app with real pointer input under xvfb, Blockly's radius, an *if* from the toolbox, a 20-unit grid over ±60 units, 49 points × 2 approaches, 98 drops per zoom, about 4.5 minutes per zoom): at 0.5 / 0.9 / 1.5, 0 mismatches, 0 swaps and 0 insertions; the preview changed on the settle move before the release (lag) in 6 / 3 / 1 drops; 34 drops wrapped the statement below the top (the second, third or fourth print) and 64 were out of reach. A loose *if* dropped beside `main` took a second Ctrl+Z in 7 / 8 / 5 of those 64: Blockly bumps it away from `main`'s connections after the drop, which the undo history records as a step of its own (a loose drop, not a wrap). An earlier run with a loose *if* already on the canvas (window 1600 × 1000) gave the same: no mismatch, swap or insertion, lags 6 / 3 / 1.
  - **Windows (WebView2):** not measured here (no Windows host). The E2E job on `windows-2025` runs the same spec; with `B2C_E2E_WRAP_SWEEP=1` set for its e2e step (PowerShell: `$env:B2C_E2E_WRAP_SWEEP = '1'` before `pnpm --filter @blocks2cpp/desktop run e2e`), the zoom test sweeps instead and writes `wrap-sweep.json` with the counts to the job's uploaded artifacts. The happy-dom sweep is the same on Windows (`$env:B2C_WRAP_SWEEP = 'top'` and `$env:B2C_WRAP_SWEEP_ZOOMS = '0.9'`, then `pnpm --filter @blocks2cpp/desktop exec vitest run src/editor/sync/wrapSweep.test.ts -t measures --disableConsoleIntercept`; without that flag Vitest shows no table).
  - **Integration (2026-10-15).** Merged after W5-1: `dragWrap.test.ts` and `wrapSweep.test.ts` pass with W5-1's radius and preference. In WebKitGTK the zoom test's fixed drops, brought down from 120 px above the gap, kept wrapping the place they passed at zoom 0.5 (W5-1's integration finding: the preview of the place above pushes the aimed statement down, and that place stays in reach); they now come in from the right at the gap's height and pass at the three zooms, and the sweep keeps both vertical approaches. The WebKitGTK sweep of the merged build (`B2C_E2E_WRAP_SWEEP=1`, 98 drops per zoom, W5-1's radius) at 0.5 / 0.9 / 1.5: 0 mismatches and 0 insertions at each zoom, as before; out of reach 0 / 28 / 28 drops (64 with Blockly's radius); lags 8 / 11 / 7 (6 / 3 / 1); and the two approaches dropped differently at 37 / 13 / 13 of the 49 points (0 with Blockly's radius), which is W5-1's integration finding. The first test's move away from the preview also left an 800 × 560 window (300 px right and 220 px down); it now moves right at the drop's height, inside the window.
  - **Review fix (2026-10-15): the sweep's radius.** The table's rows were measured in the wrapping package before W5-1 merged, where the drag strategy read Blockly's configuration. After the merge the sweep still set only that configuration, which W5-1's strategy does not read (it returns its own radius and sets its own preference for each move), so both radii measured W5-1's: the review's re-run at 40d81be gave `print`, *Blockly (28, 8)*, 0.5 as 585 of 625, the W5-1 row's numbers. The sweep now gives each radius to the strategy through the dragger (`B2cBlockDragger.dragStrategyFor`, `B2cBlockDragStrategy`'s `SnapReach`) and leaves Blockly's configuration (and so bumping, 28 units) alone. Re-measured at zoom 0.9 after the fix: `print` gives the table's rows exactly (Blockly 11 of 625, edge flickers 31, lags 46; W5-1 259 of 625, 8, 85), and `top` within a few drops of them (Blockly 12 of 625, 25, 51; W5-1 251 of 625, 8, 101; the package's W5-1 rows also bumped with W5-1's 48–96 units). The other rows were not measured again.

**W5-3 Placement E2E.** Ref: U8d.

- **Change:** `apps/desktop/e2e/specs/flows/placement.e2e.ts`, with real pointer input. It covers:
  - a drop off-target into a mouth;
  - a drop on the flyout during a preview, including a release with no final move;
  - a drop on the trash can, then Ctrl+Z;
  - *Show all* keeps the zoom.
  - The wrap cases are enabled with W5-2.
- Effort M.
- *Status (2026-10-15):* done. `specs/flows/placement.e2e.ts`, with real WebDriver pointer actions: a *print* dropped with its top 32 px right of and 16 px above an empty *repeat* mouth (about 36 px away, where Blockly's 28 units are 25 px at 90 % and 15 px at 52 %) lands in it at 90 % and after three *Zoom out* clicks (52 %), and *Show all blocks* then keeps the zoom; a *print* dragged until a place between `main`'s first two statements is previewed, then dropped on the flyout after moving over it, on the flyout with the jump and the release in one action, and on the trash can, deletes only itself each time, and Ctrl+Z puts it back. A release with no pointer move at all stays covered by `drag.test.ts` (a WebDriver action always moves before it releases). The wrap cases are `placement-wrap.e2e.ts` (W5-2). The snapping test fails on a bee5c0a build. Integration: both specs pass on Linux at 800×560, 1024×700, 1028×749 and 1280×800 (the full Linux suite at the default size; the flows and exit specs at the other three).

**W5-4 Cheap pre-drag snapshot.** Refs: U8e, U11-3.

- **Cause:** `EditorSession.beforeDrag` reads the whole canvas (`editor/sync/session.ts:370-381`). That is 3.3 s at drag start at 1,000 blocks.
- **Change:** reuse the committed document when the pipeline is idle (`PreviewPipeline.isIdle()`).
- **Tests:**
  - A spy on `readTopBlocks`: no read when nothing is pending, a read when an edit is pending.
  - A save during a drag still writes the pre-drag canvas.
- Effort S.
- *Status (2026-10-15):* done. `PreviewPipeline.isIdle()`: no run waits or runs, and the latest run's read loaded and is the project's document (a private `committed` sequence, set where a run commits). `EditorSession.beforeDrag` then keeps the shown module's blocks from `project.document` and reads the canvas only otherwise. *Deviation:* the premise no longer held at bee5c0a: the read costs about 10 ms at 1,000 blocks (`b2c:preview:read`), not 3.3 s; in the package's measurement the drag start's longest task fell from about 45 to 33 ms (medians over all drags 61 → 36 ms). Limit, tested and documented: a change Blockly has made but not announced yet (its events come after the next frame) is not in a save during that drag, and the project stays unsaved after the drop. Tests: `pipeline.test.ts` (`isIdle` through a run, a debounce, `invalidate`, a canvas that does not load and a read put off during a drag) and `drag.test.ts` (a spy on `readTopBlocks`: no read when idle, one when an edit waits, the toolbox block both ways). With P-1 merged the pipeline commits when `update()` answers, so a drag that starts while an update runs reads the canvas; the integration rewrote the `isIdle` test for that. The A/B's drag benchmark drags a definition across empty canvas and has no drag-start metric, so it does not show this change (`drag-5000.frame-p95` 62 → 61.5 ms).

### W6 — Documentation (rest of U18)

`BUILDING.md` moved to W1-18.

**W6-2 Split the desktop README (if time).** Ref: U18-2.

- **Change:**
  - `apps/desktop/README.md` becomes about 50 lines: what the app is, a code map, and links.
  - New `docs/developer-guide/`: `desktop-architecture.md`, `desktop-security.md`, `dependencies.md`, `testing.md`, `recipes.md`.
  - The test IDs move to `e2e/README.md`.
  - Update the inbound links: `packages/README.md:75` and the Cargo comments.
- **Tests:**
  - The W0-5 anchor check, markdownlint and `pnpm site:build`.
  - A sentence-bag script showing content was moved, not lost.
- Effort M. Depends on D7.
- *Seam entries from W5 (2026-10-15), for the guide's list of seams (ADR-0014), all checked against Blockly 12.5.1:* `B2cConnectionPreviewer` extends `InsertionMarkerPreviewer` (overrides `previewReplacement`, `previewConnection` and `hidePreview`; uses the protected `serializeBlockToInsertionMarker` and `renderManagement.triggerQueuedRenders`); `B2cConnectionChecker.doDragChecks` allows the wrap pair and reads the public `Blockly.common.draggingConnections`; `B2cBlockDragger.onDragEnd` uses the protected `Dragger.shouldReturnToStart`; `B2cBlockDragStrategy.drag` writes the public `Blockly.config.currentConnectionPreference` for the length of each move.

**W6-3 README and CONTRIBUTING.** Refs: U18-3, U18-10, RD-14.

- **Change:**
  - The README gets a 3-line status note, and "What works today (M2)" is separated from "Goals for 1.0".
  - CONTRIBUTING:
    - set-up links to `BUILDING.md`;
    - one consolidated list of checks;
    - the crate list is fixed;
    - "Where things are documented".
- **Tests:**
  - The anchor check and markdownlint.
  - A reviewer runs the consolidated check list on a clean clone set up with `BUILDING.md`.
- Effort S.

**W6-4 Docs consistency check (if time).** Ref: U18-4.

- **Change:** `tools/check-build-docs.py`, run in the docs job, checks:
  - that the pinned versions (wasm-bindgen, Rust, Node, pnpm, tauri-driver) and the apt list match their sources;
  - that no other document repeats them.
- **Tests:** `--self-test` with fixtures that break each rule, plus a scratch wasm-bindgen bump that fails.
- Effort S.

**W6-5 Stale WASM guard (if time).** Ref: U18-6.

- **Change:**
  - `pkg/build-info.json` holds a SHA-256 of the inputs.
  - The Vite plugin fails a production build when it is stale, and warns in dev.
  - Root scripts `wasm:build` and `app:build`.
  - Optional: a start-up check comparing the core's catalog version with `app_info.catalogVersion`.
- **Tests:**
  - The fingerprint is stable, and it changes when a covered file changes.
  - A stale build-info file fails the Vite plugin.
  - With the runtime check, `bootstrap.test.ts` shows the blocking error.
- Effort S.

### Other performance items (after W2-1, in parallel with W3–W5)

**P-1 Single `update()` export (if time; otherwise first in M3).** Refs: U11-4, A04-06.

- **Change:**
  - Load and hash the document once.
  - The canonical text is produced on demand, for save, build, autosave and external-change checks.
  - Dirty = hash ≠ saved hash.
- **Expected saving:** about 30 ms per edit at 1,000 blocks.
- **Tests:**
  - The same preview and hash for every example and the security corpus.
  - One WASM call per run.
  - Save, build and autosave still receive exactly the canonical text.
- Effort M.
- *Status (2026-10-15):* done. The core's `preview()` export is now `update(documentJson, optionsJson)`: it loads the document once, runs the pipeline on it and returns the preview with `textHash`, the SHA-256 of the canonical text (`null` when the document does not load); `canonical()` returns `textHash` too. The pipeline makes one `update()` call per run and commits the document with `textHash` and `contentHash` in one store update; dirty = `textHash` ≠ `savedTextHash`. *Deviations:* dirty uses the new text hash, not the 05 §5.11 content hash, which leaves out the layout (a moved block would not count as a change); the store holds no canonical text (`canonicalText`/`savedCanonicalText` became `textHash`/`savedTextHash`), so save, build and autosave make it with `canonical()` when they need it; an update that fails twice commits the read through `canonical()` alone, so an edit the analysis cannot handle stays unsaved and autosaved; the core keeps its last analysis when an update's document fails to load (the scope query keeps answering). Spans: `b2c:preview:update` replaces `:canonical` and `:analyse`. Tests: the Rust facade, every example and the security corpus (update equals `preview_document`, `textHash` equals the SHA-256 of `canonical().text`), a spy test of one `update` and no `canonical` per run, `pipeline.core.test.ts` with the real core (a moved block is dirty with the content hash unchanged), autosave, the build fallback, external change. Measured by the package (Vitest, real core, 1,000 blocks, medians): `b2c:preview` 87–94 → 69–75 ms; core work per edit 88 → 52 ms in an interleaved A/B; WASM 1,071,756 bytes after `wasm-opt` (+1,455). Integration A/B (the same run as W5-1's): `preview-1000.p95` 113 → 92 ms (−18.6 %) and `edit-1000.p95` 1,580.5 → 1,449 ms (−8.3 %); per edit at 1,000 blocks `b2c:preview` 72–76.5 → 56–58 ms (`analyse` 35–38.5 ms plus `canonical` 23–23.5 ms became `update` 42.5–44 ms, `read` 8 ms either way); guessing game `edit.max-task` 38.5 → 36 ms (its `b2c:preview` 11–12 → 6–8 ms). No gated metric regressed: `select-1000.max-task` 1,002 → 1,020 ms (+1.8 %), guessing game `select.max-task` 17 → 18 ms.

**P-2 Less churn per preview.** Refs: U11-5, FA-6.

- **Change:**
  - Call-argument labels come from a map built once per preview, not one WASM scope query per call block.
  - The autosave check compares fields, not concatenated strings.
  - At most 2 store updates per run.
  - Narrow selectors (`useShallow`).
  - The Problems grid and the C++ panel skip work while hidden, and refresh once when shown.
- **Tests:**
  - Counters: 0 `symbolsInScope` calls from labels with 50 call blocks; at most 2 store notifications per run; no `buildProblemItems` while the Console tab is shown.
  - React Profiler render counts.
- Effort S.
- *Status (2026-10-12):* done. The A/B was run locally: base 9c098e9 (W2-1 without P-2) and head (with P-2), 2 rounds of 5 samples in alternating runs (10 per side), debug backend, Linux under xvfb.
  - `preview-1000.p95`: 120.5 → 101.5 ms (−15.8%), an improvement past the 10% threshold.
  - `edit-1000.p95`: 1,338.5 → 1,411.5 ms (+5.5%), within noise; Blockly's rendering dominates.
  - `select-1000.max-task`: 1,001 → 999 ms (−0.2%). Guessing game: `select` 15 → 15 ms; `edit` 40.5 → 36.5 ms (−9.9%, just under the threshold). These selection readings skipped the toolbox's rebuild on both sides (W2-1's review fixes). Taken again on 2026-10-13 with the rebuild after the click on both sides (base 9c098e9 with the toolbox fix, head with P-2 and the review fixes; the A/B rounds now click all 8 scopes): `select-1000.max-task` 1,126 → 1,108.5 ms (−1.6%), guessing game `select` 145.5 → 149.5 ms (+2.7%) and `edit` 33.5 → 34.5 ms (+3.0%).
  - No gated metric regressed. Per edit at 1,000 blocks, `b2c:preview` takes about 66 ms (`analyse` 32, `canonical` 22, `read` 7) and the code panel about 4 ms.

**P-3 Render-management spike (if time).** Ref: U11-2.

- **Change:** patch Blockly's render management and make `getDescendants` linear, in a local branch, under ADR-0014. Open an upstream issue.
- **Exit criterion:** a measured result recorded for D6/D15: the render count when appending to a 500-statement chain, and the `edit-1000` time with and without the patch. The patch itself is M3-2.
- Effort S.
- *Status (2026-10-15):* done, as a measurement only: nothing in the app or its dependencies changed. The patch is [`upstream/blockly-render-management.patch`](upstream/blockly-render-management.patch) (against Blockly 12.5.1's `core/render_management.ts` and `core/block.ts`); as measured, the same change in the npm package's `blockly_compressed.js` is [`upstream/blockly-render-management.bundle.patch`](upstream/blockly-render-management.bundle.patch), applied to a local install only. The upstream issue is drafted in [`upstream/blockly-render-management.md`](upstream/blockly-render-management.md) and not filed (F-8).
  - **The patch.** `queueBlock` walks up iteratively and marks for drawing only the queued block, its parent, and each block further up that the walk reaches through one of its inputs or that has an output connection; a block reached through its next connection is only walked through, because a block's shape does not depend on the blocks below it. `renderBlock` still walks every dirty block but draws only the marked ones; `doRenders` updates component locations from each drawn block down; `getDescendants` walks with a stack of its own, in the same order.
  - **Render count** (Vitest with happy-dom, Zelos, the app's blocks; a print appended to 500 prints in *when program starts*): 503 → 4 blocks drawn (the new print and its shadow, the print above it, and `main`, whose mouth grew); deleting it again 501 → 3. Component locations (1,003 blocks) and connection moves (2,005) are unchanged, because `main` is drawn and its whole subtree is updated.
  - **Correctness.** A property test of 40 random edits each with Zelos and Geras (append, insert after a statement, insert into a mouth, delete with healing, move into another mouth, collapse and expand): after each edit's render, every block's place, size and outline, every connection's place and the connection databases equal what `workspace.render()` gives. `getDescendants` keeps the recursive order.
  - **`getDescendants`** on a headless chain: 1,000 statements 4.0 → 0.21 ms, 5,000 188 → 0.36 ms, 10,000 681 → 0.90 ms, 20,000 a stack overflow → 1.55 ms. Blockly's `dispose` still recurses and overflows from 5,000 statements in the same test. At the benchmarks' sizes the gain is small: on a rendered 150-statement chain (301 blocks) a call takes 0.15–0.16 ms either way.
  - **A/B in the app** (local, Linux under xvfb, debug backend, E2E builds of bee5c0a without and with the bundle patch, 2 rounds of 5 samples in alternating runs, 10 per side): `edit-1000.p95` 1,627.5 → 1,071 ms (−34.2 %); `select-1000.max-task` 924.5 → 991.5 ms (+7.2 %, within noise: each target costs about the same on both sides, 48–156 ms for the three inside a loop (two changes and an `if`), 0.7–1.9 s for the five in `main`'s body or in a print); `preview-1000.p95` 115.5 → 110 ms (−4.8 %); guessing game `edit` 35 → 35.5 ms, `select` 17 → 17 ms.
  - **Where the time goes** (diagnostic builds with timing probes in Blockly's render pass and in `Gesture.setTargetBlock`; 8 edits and 16 clicks per build, medians). An edit at the end of `main` draws 382 → 4 blocks and its drawing takes 439.5 → 20 ms (a deletion 442.5 → 3 ms); `resizeContents` stays at about 210 ms (216 → 204), `bumpNeighbours` at 30–40 ms and component locations at 11–13 ms; the page's longest task falls from 1,110.5 to 729.5 ms. A click draws nothing: its cost is `BlockSvg.bringToFront`, 0.3–0.9 s for four of the eight targets (a print and a loop in `main`'s body, a getter in a print, a declaration), each re-appending one to three SVG groups, and a statement's group holds every block below it in its stack; under 35 ms for the others. `focusNode` adds 21–181 ms, growing along the stack. Both are the same with and without the patch.
  - **Integration (2026-10-15):** merged as documents only (the plan and `docs/plans/upstream/`); the app still ships Blockly 12.5.1 unchanged, and the patch waits for M3-2.
  - **The spike's tests and tools** are kept in [`upstream/p3-spike/`](upstream/p3-spike/README.md) (review fix, 2026-10-15): `renderSpike.test.ts` (the render count, the placement property test and the `getDescendants` checks), `patch_bundle.py` (gives the bundle patch's recorded SHA-256), `instrument_bundle.py` and the E2E probe `p3probe.bench.e2e.ts`, with how to run each. They are not part of the app's suites.

### W7 — M2 close-out

**Nightly 17 (2026-10-14, run 38538265801, commit `bee5c0a`).** Three jobs failed, with four failures; the code involved is unchanged at `d4dfafb`. The fuzz targets, the link check, the Linux E2E jobs and the Windows benchmarks passed. The next nightly (2026-10-15, run 38705834556) ran `d4dfafb` and passed every job.

- *E2E (windows-2025), `rename-project` step (4):* a Ctrl+S pressed about 330 ms after the first *Save as…* had written the file never saved. `runCommand` joined it to the `project.save` still running, whose promise settles only after the rest of the backend's Save as (trust.json and recent.json written durably, the first file watcher started, the snapshot dropped) and the `trust_get` after it, so nothing ran it again. Likely cause, not proven: the logs fit, and nothing else does. In the app, an edit saved at once while a save was finishing was dropped without a word on a slow disk; the test missed it because a new template project has no unsaved changes, so step (1)'s marker check passed before the Save as had finished. *Fixed:* a Save asked for while one runs follows it, and the requests made during one save make one save (`beaf1d8`); step (1) waits for *Saved* in the status bar (`71ff6d0`).
- *E2E security (windows-2025), `ipc-abuse.e2e.ts` test 3:* `waitUntilReady` gave up after 30 s. msedgedriver held one `ready()` call for 34.2 s on a pending navigation, and the page loaded at about 34.5 s; nothing in the app delays the page. Every E2E launch shared one web view data folder (Tauri's default, the app's local data folder, which `B2C_E2E_ROOT` did not move), and test 3 started 0.3 s after test 2's app exited; whether that slowed the page is not known. *Fixed:* the ready wait is 60 s, the page-load timeout, and its message names the phase the start reached (`8fd959e`); E2E builds keep the web view's data in the test root's `webview/` (`76f3a7e`).
- *Benchmarks (ubuntu-24.04):* two gated metrics were more than 10 % worse than the median of the last 5 runs in the history (`0709960`..`9e9bd35`): `native.pipeline.preview` 17.23 against 15.49 ms (+11.2 %) and `webview.drag-5000.frame-p95` 49 against 36 ms (+36.1 %). Neither is a code regression. Callgrind counts one `preview_document` call at 335,093,419 instructions at `36a5e5d` (the last commit that passed this gate, nightly 16) and 340,085,175 at `d4dfafb` (+1.5 %: load +0.1 %, analysis +1.4 %, generation +1.0 %), while every native metric of that run was 4–15 % slower: a slower runner. The GitHub A/B run 38693442739 (base `43ad7bc`, head `d4dfafb`, the same runner, alternating, 10 samples per side) measured drag-5000 at 43 → 45 ms (+4.7 %, within the gate), and the next nightly measured 15.83 ms and 39 ms. That A/B also flagged `webview.edit-1000.p95` +24.3 % on Linux, which is tracked separately.

**W7-1 Chrome visual diff (if time).** Ref: RD-2.

- **Scenes:**
  - the ≡ menu open;
  - the toolbar select, closed and hovered;
  - the console after two runs;
  - the toolbox after clicking each category;
  - the 800×560 window;
  - the console scrolled.
- **Themes:** light on both systems; dark on Linux through `GTK_THEME=Adwaita:dark`.
- **Acceptance:**
  - The `elementFromPoint` stacking E2E and the toolbar contrast crop landed in W1 and fail on HEAD.
  - The pixel diff starts comparing once W7-2 commits reviewed baselines.
- Effort M.

**W7-2 Commit reviewed visual baselines.** Refs: RD-1, RD-8e.

- **Today:** `e2e/visual/baselines/` holds only a README.
- **Change:**
  - Take candidates from a green nightly after the must-have W1–W4 changes, review them, and commit them for both systems.
  - Correct 10 §10.1 and 09 §9.3.
  - Note that the benchmark history lives in a per-branch Actions cache (`nightly.yml:888-913`) and must move before 1.0 changes the default branch.
- **Tests:** a throwaway colour change fails the job on both systems.
- Effort S.

**W7-3 Record the M2 threat-model review.** Ref: RD-5.

- **Change:** a new record in `docs/security/threat-model-reviews.md` covering:
  - the IPC commands, the isolation allowlist, and `settings_update{ui}` if W3-4 landed;
  - `unsafe` in `b2c-process`;
  - the stores;
  - the Trusted Types trial results;
  - the licences of new dependencies.
- Effort M.

**W7-4 A green weekly run.** Ref: RD-6. Dispatch `weekly.yml` by hand; the only run so far came before the fixes. Effort S.

- *Finding (2026-10-13, with W3's A/B work):* the scheduled Weekly runs at 9e9bd35 (38256411360, 38256451584) and the run at 205ab1d (37310642141) failed only in mutation shards, each ending "The runner has received a shutdown signal" 17–37 minutes into the shard. Some mutants turn a bounded loop into an endless one that allocates: `crates/b2c-toolchain/src/flags.rs:313` `index += 1` → `*= 1` in `filter_pkg_config` dies in 5 s under a 2 GiB address-space cap (reproduced locally) and, uncapped, fills the hosted runner's 16 GB within the per-mutant timeout. Several `b2c-model` `json.rs` mutants (`+=` → `*=`/`-=`) loop without allocating and cost a full timeout each (reported only).
- *Finding (W3 review):* W3-10 rewrote `crates/b2c-toolchain/src/discovery.rs`, which is in the mutation scope. The `#[cfg(not(unix))]` `is_executable` moved from line 386 to 633, out of its line-pinned `exclude_re` entry, and the new `home_folder` had no test that saw its value: `cargo mutants -f discovery.rs` missed 3 mutants beyond the excluded `is_network_path` one (`home_folder` → `None` and → `Some(Default::default())`, `is_executable` → `false`), so the Weekly would stay red after the memory fix. *Fixed:* `is_executable` is one function with a `cfg` block per platform (the tested Unix body is the one mutated on Linux), and the line-pinned entry is gone (`.cargo/mutants.toml` now says never to pin a line); `home_folder` reads its variable through `home_folder_in`, unit-tested for both platforms (absolute, relative, unset) and against the process's own `HOME`.
- *Proposed fix (not applied):* cap each test process, not the whole run: a `prlimit --as=<limit> --` target runner (or `ulimit -v`), under which the runaway allocation aborts the test, so the mutant counts as caught (the `flags.rs:313` mutant dies in 5 s under 2 GiB). A memory-capped systemd scope around `cargo mutants` (`systemd-run --user --scope -p MemoryMax=12G -p MemorySwapMax=0 …`) is the second choice and needs `-p OOMPolicy=continue`: with the default `OOMPolicy=stop` (systemd 255 on ubuntu-24.04), the first OOM kill stops the whole scope, `cargo mutants` included, and the shard ends without its report. The weekly job would also need the user-manager setup that `ci.yml`'s test-cgroup job does before its own `systemd-run --user --scope` (enable lingering, wait for `user@<uid>.service`, set `XDG_RUNTIME_DIR`); that job runs no capped scope itself. Either way, shorten the per-mutant timeout (`--timeout` or `timeout_multiplier`); then dispatch `weekly.yml`.

**W7-5 Toolchain matrix.** Refs: RD-7, D16.

- **W7-5a (must, S):** the owner's Debian g++ cell, following a new `docs/manual-tests/m2-toolchains.md`.
- **W7-5b (if time, M):** the other cells, or a recorded deferral to M3.

**W7-6 Usability sessions, NVDA/Orca pass and demo exclusion review.** These run on a polish-wave release build. Effort M (owner time).

**W7-7 IPC-abuse `app_quit` follow-up (if time).** Ref: RD-9.

- **Change:**
  - Document the incident and the *likely* cause in `apps/desktop/e2e/specs/security/README.md`. The likely cause is WebDriver's conversion of the arguments.
  - Assert that the debug log has no `app_quit` span after the mutation batch.
  - `isolation-tests` cases for `{constructor}`, `{prototype}` and extra keys on every command that takes no arguments.
- Effort S.

**H-1 Housekeeping PR (if time).** One small PR for the P3 items:

| Ref | Fix | Test |
|---|---|---|
| FA-8 | Clear a stale `ui.selection` after a module switch or reload | tracking test |
| FA-9 | The C++ panel follows the module switcher | `panels.test.tsx` |
| FA-10 | Build output follows again when a new build starts | `BuildOutputPanel.test.tsx` |
| FA-14 | Run no longer stuck at *starting* on an invalid run ID | fake IPC test |
| FA-15 | *Learn more* says it opens the browser; block paths start with *when program starts* | `problemItems` and `ProblemsPanel` tests |
| S8-1 | Toolchain path in the status bar's tooltip and accessible name | StatusBar test |

### Future list

- **F-1 Appearance setting (System, Light, Dark).** M3, after a `[data-theme]` token refactor.
  - The dark blocks become `:root[data-theme]` rules.
  - The Blockly themes and the console tokens follow the setting (U14d).
  - `cssAudit` understands `[data-theme]`.
  - The Settings page gets a radio group.
  - It also gives the visual diff a dark-theme seam on WebView2 (RR-4).
- **F-2 High Contrast themes and a checked colour-blind-safe palette.** M5.
- **F-3 Remember the window's size and position.** M5.
- **F-4 A keyboard equivalent of wrapping.** M5.
- **F-5 "Clear the console when a run starts".** M3, if D5 chooses (c).
- **F-6 A toolbox block-size setting (S/M/L).** Later (D9).
- **F-7 A build counts as up to date when its generated files are identical.** U5-e, M5. Until then, editing a parked block makes the next Run recompile.
- **F-8 The issue tracker and the upstream reports.** Deferred by the owner on 2026-10-12: the labels and one issue per U-item and work item (W0-7), and filing the drafts in `docs/plans/upstream/` with Blockly and blockly-samples. Until then this plan and the roadmap's status notes track the work.

### M3 work moved out of M2

- **Moved from the M2 scope:**
  - **M3-1 Preview in a Web Worker** (ADR-0016, D15), after P-1.
  - **M3-2 Blockly render-management patch and a linear `getDescendants`.** Only if ADR-0014 and P-3's measurement justify it.
  - **M3-3 Blockly 13, continuous-toolbox 13 and xterm.js 6.1.** After a spike. xterm 6.1 once a stable release exists; 6.0.0 cannot load under the app's frozen prototype. This removes W3-8's wheel workaround.
  - **M3-4 Lower parked stacks in a sandbox** for the scope query and block types (U5-d). Scopes checkpoint and rollback, a side table that codegen never reads, and `ItemCtx::Parked`. Tests:
    - a parked `create x` is visible below it in the same stack and not to `main`;
    - `block_types` contains parked reporters;
    - parked code reports no diagnostics, and the C++ is unchanged;
    - a text value no longer snaps into a parked int `set`.
  - **M3-5 Compact source map and a code panel that pauses while hidden** (U11-8).
  - **M3-6 `pnpm doctor`** (U18-7).
  - **M3-7 A weekly job that runs `BUILDING.md`'s Debian block** in Debian 12 and 13 containers (U18-9).
  - **M3-8 Type-specific variable operations,** and the bitwise compound forms with *Show advanced blocks*.
  - **M3-9 Small-program budgets gated** once the section-level toolbox refresh and a measured baseline exist.
  - **F-1.**
  - **Every if-time item not done in M2.**
- **Language foundations, from the language audit:**
  - L14: an open type representation (XL);
  - L15: the slot-typing pipeline (XL);
  - L17: catalog schema extensions (L);
  - L16: `text + "x"` where C++ allows it;
  - L10: constant folding for overflow and division by zero;
  - L12: scheduling `switch`, do-while, `wait`, program arguments, overloads and default arguments;
  - L18: the multi-module header split;
  - L8: the catalog's `headers`.
- **From the runtime audit:**
  - S7-5: `run_start` reuses the build's analysis;
  - S7-7: sandbox folders are pruned;
  - S7-8: open-ended `g++-N` names.
- **Smaller items:**
  - U14d: console tokens follow the theme (with F-1);
  - RD-15: the skipped-lines marker on Windows.

## 4. Additional findings beyond the owner's list

### P0 and P1

| ID | Finding | Fix | Pri | When |
|---|---|---|---|---|
| R-DROP-DELETE / U8c | A drop on the toolbox while previewing a connection deletes the stack below the insertion point; a release without a final move keeps a stale candidate | W1-0a | P0 | W1 |
| L1 | `-1` typed into a slot gives E0310 | W1-0b | P0 | W1 |
| A04-01 | The toolbox shows a renamed variable's or function's old name | W1-10 | P1 | W1 |
| L4 | *keep asking until valid* accepts `3.5` as 3 and `12abc` as 12; the text ask skips empty lines silently (`helpers.rs:75, 149-151`) | W1-12 | P1 | W1 |
| FA-1 | Console output is invisible to screen readers (`ConsolePanel.tsx:219-232`) | W3-11 | P1 | W3 |
| L2 / U5-b | `b2c check` stops at catalog errors and hides analyser problems that the app shows | W4-3 | P1 | W4 |
| L3 / U2-c | The scope query answers `[]` for loose blocks and for blocks inside disabled statements | W1-6, W4-4 | P1 | W1, W4 |
| RD-1 / RD-2 | The visual diff never compared anything; the chrome and its appearance are untested | DOM guards in W1-1/W1-2; W7-2; W7-1 if time | P1 | W1, W7 |
| RD-3 | Dependabot groups block patches behind majors | W0-6 | P1 | W0 |
| RD-5 | No threat-model review is recorded for any milestone | W7-3 | P1 | W7 |
| RD-14 / U18 | Build steps are scattered, and the README describes 1.0 goals in the present tense | W1-18, W6-3 | P1 | W1, W6 |
| Review | The spec is inconsistent: a duplicate goal ID (N10), conflicting format-version rules, a "compatible change" claim the editor breaks, upgrades not ordered against trust, Blockly extensions beyond ADR-0002 | §5 items 3–6, 8, 26, 39, 53, 56; W0-3, W0-4 | P1 | W0 |

### P2

| ID | Finding | Fix | When |
|---|---|---|---|
| U3-3 | Block migrations never reach the editor | W4-1 (if time) | W4 or M3 |
| C1 | Blockly pop-ups are drawn over modal dialogs | W1-1 | W1 |
| FA-3 | The trash can keeps blocks from earlier projects and restores them through Blockly's own serialisation | W1-14 | W1 |
| FA-4, A04-07, RD-13 | *Inline/External Inputs* is not saved; *Delete N Blocks* counts shadows, with OK as the default | W1-15 | W1 |
| S7-3 | Run during the first compiler search opens the setup page | W1-16 | W1 |
| L5 | `join` shows 2.5 as `2.500000` | W1-13 | W1 |
| L6 | Change menus offer loop counters, read-only parameters and text variables | W1-19 | W1 |
| L7 / U3-4 | Checkboxes and the print stream have no label; the character block is called "letter" | W1-17 | W1 |
| S5-2 | M3/M5 keys give "unknown key, remove it" in M2 | W0-3 | W0 |
| L11, L13, A04-20, A04-21, S7-4, S5-3, S5-4 | Spec text reads as current where M2 differs | W0-8 | W0 |
| FA-2 | No keyboard zoom and no Ctrl+wheel zoom | W3-5 | W3 |
| FA-5 | No visible Undo; the banner's Ctrl+Z works only with focus on the canvas | W3-9 (if time) | W3 |
| S7-2 | Started from `/`, discovery finds no g++; started from `$HOME`, it skips `~/.local/bin` | W3-10 (if time) | W3 |
| RD-10 | A transient "no usable g++" on a Windows first start (*hypothesis*: probe timeout) | W3-12 | W3 |
| U14c | Refitting on every resize reflows the whole scrollback | W3-8 | W3 |
| FA-6 | Hidden panels redo their full work on every edit | P-2 | W3 period |
| FA-7 | *when program starts* is always offered, but a second one is E0406 | W4-6 | W4 |
| U2b | The Variables section shifts later categories by 312 units per variable | W4-2 | W4 |
| L9 / U5-c | Messages name blocks by catalog ID ("io.print") | W4-3 | W4 |
| RD-6 | The weekly mutation gate has never been green | W7-4 | W7 |
| RD-7 | The toolchain-matrix protocol has not been written | W7-5 | W7 |
| RD-9 | Unexplained `app_quit` in the Windows IPC-abuse test (*likely* WebDriver argument conversion) | W7-7 (if time) | W7 |
| RD-12 | No issue tracking | W0-7 | W0 |
| U18-4, U18-6 | Pinned versions drift between docs; a stale WASM core after `git pull` goes unnoticed | W6-4, W6-5 (if time) | W6 |

### P3 (housekeeping and spec wording)

- **Code, in H-1:** FA-8, FA-9, FA-10, FA-14, FA-15, S8-1.
- **Spec wording only, in W0-8:**
  - S5-3: trust is evaluated at open, not at build;
  - S7-4 and S5-4: small inaccuracies in 07 and 05.
- **Decided in D16:** S5-6, the permissions of new project files.
- **M3:** S7-5, S7-7, S7-8, L8, U14d, RD-15, U11-8, U18-7, U18-9.
- **M5:** U5-e, the up-to-date check by generated files (F-7).
- **M6:** RD-11, enforcing Trusted Types.

### Not adopted

| Proposal | Reason |
|---|---|
| Relabel `logic.ternary` (U7-2) | Nobody reported a problem; revisit with the C++ label mode (M3) |
| A *run without using the result* call form (U7-2) | Decide in M3 (D11) |
| Dark-theme visual diff on Windows (RR-4) | Dark is compared on Linux through `GTK_THEME`; WebView2 dark waits for F-1 (M3) |
| Always clear the console on a run (A04-09) | It loses the earlier run; it can come as an option (D5) |
| Save the inline/external state in the project file | A format change for a purely visual option |
| `tauri-plugin-window-state`, or custom monitor code | `preventOverflow` does the job; remembering window geometry is M5 |
| A global `snapRadius` | It also widens bumping, which pushes loose blocks further away |
| Patching `@blockly/continuous-toolbox` or xterm's minified viewport | The subclass slack (W1-7) and the wheel handler (W3-8) are smaller |
| Patching Blockly's private `resetZoom` | The controls would stay inaccessible |
| Asynchronous re-targeting, or re-targeting getters | Undo-group races; silent variable swaps |
| A setup script that runs `sudo` installs | Privileged installs from the repository are a security concern; `BUILDING.md` gives copy-paste blocks |
| Committing `pkg/` | Build output in git, against ADR-0010's build-time embedding |
| Blockly's gear mutator for *if*; folding *forever* into the loop menu | D4 |

<!-- markdownlint-disable ol-prefix -->

## 5. Spec and ADR revisions

**[Owner]** marks a product decision that needs the owner's approval. *Correction* marks text that only describes existing behaviour; corrections land in W0-8. Every other revision lands in the same PR as the code it describes.

### New ADRs and changes to existing ADRs

1. **[Owner] New `docs/adr/0012-parked-blocks.md`: "Loose blocks are parked: a warning, not an error".** Status: "Accepted (owner decision D1 of 2026-10-10)": a warning, no dimming, E0406 stays, and the parked-stack limitation is accepted until M3.
   - **Decision:**
     - (1) "A top-level block whose catalog shape is statement, reporter or predicate is parked, together with its `stack`. It is saved and shown, but never lowered, generated or run, and being loose never stops a build. Definitions (*when program starts*, *define*, and later struct, class, enum, global and Raw C++ declarations) are never parked."
     - (2) "The analyser reports each enabled parked head once as the lint `B2C-W0505` (default: warning; a project can raise it to an error once lint levels exist, M5). A parked head the user disabled is not reported."
     - (3) "`B2C-E0604` remains for the placements the editor cannot produce: a hat or definition inside a block, a statement in a value input, a reporter as a step, and blocks stacked below a block that is not a statement."
     - (4) "Parked blocks are checked against the catalog like every other block. Every catalog check except placement (an unknown or newer block type, a block version that cannot be upgraded, and an unknown, missing or invalid field, input, `extra` setting or parameter: `E0601`–`E0603` and `E0605`–`E0614`) stays an error wherever the block is, so a damaged or unsupported block (including a Raw C++ block before M4, 08 §8.3.1) still stops the build."
     - (5) "Parked code is not analysed until it is attached. The scope query answers parked blocks with the parked scope (06 §6.5)."
   - **Consequences:**
     - "Run works with parked code; `b2c check` exits 0 with a warning; the generated C++ is identical with or without parked blocks."
     - "Until parked code is lowered for the scope query (M3), a variable created inside a parked stack is not offered by the menus of the blocks below it, and is not listed in Variables. A parked reporter also has no known type, so the connection checker accepts any value in its slots. Both resolve when the stack is attached."
     - "Editing a parked block changes the project's content hash, so the next Run recompiles although the C++ is unchanged, until 07 §7.5.1 compares the generated files (M5)."
     - "ADR-0011's objections are answered by the visible warning, by lint levels (M5) and by the round-trip tests that the editor never detaches a stack by itself."
2. **`docs/adr/0011-loose-blocks-in-m2.md` and `docs/adr/README.md`.**
   - ADR-0011 status: "Partly superseded by ADR-0012: the placement decision (`B2C-E0604` stays an error; option 5 rejected). The `stack` key and the clipboard shape stay in force."
   - README template status line: "Status: Proposed | Accepted | Partly superseded by ADR-XXXX (names the superseded points) | Superseded by ADR-XXXX".
   - Index row 0011: "Loose stacks are saved intact (placement decision superseded by 0012)", status "Partly superseded by 0012".
   - New rows for 0012–0014, status Accepted.
3. **[Owner] New `docs/adr/0013-additive-format-keys.md`: "Additive format keys raise `formatVersion` only when they are used".** It amends ADR-0004 and notes ADR-0011, whose `stack` key stays in format 1. Status: Accepted (D18, decided 2026-10-10).
   - Decision: "`formatVersion` is the lowest version whose keys cover the file. Each version only adds keys and never changes the meaning of earlier ones: version 2 adds `options.usingNamespaceStd` (M3), version 3 adds `project.lints` (M5), and each later key gets the next number. The writer computes the version from the keys present, so a file that uses no newer key keeps its version and still opens in older apps. Only a change to the meaning or shape of an existing key needs a migration v(n) → v(n+1), and `migratedFrom` reports only such migrations. An unknown key in a file whose `generator.app` is newer than this app is `B2C-E0108` (*made with a newer version of Blocks2Cpp*, without naming a version); otherwise it is `B2C-E0110`."
   - Consequences: "Every reader that accepts `lints` enforces the W0520 rule. `b2c migrate` migrates to the lowest version that covers the file."
4. **[Owner] New `docs/adr/0014-extending-blockly.md`: "Extending Blockly: allowed seams and patched dependencies".** It supersedes ADR-0002's clause "extended through supported plugin APIs only". Status: Accepted (D6, decided 2026-10-10).
   - Decision:
     - "The editor may use the public and protected methods of documented Blockly and plugin classes (for example `BlockDragStrategy.drag` and `getSearchRadius`, `Dragger.onDrag`, `onDragEnd` and `wouldDeleteDraggable`, `Flyout.getFlyoutScale` and `reflowInternal_`, `ConnectionChecker.doDragChecks`), components replaced through `Blockly.registry` (connection checker, connection previewer, flyouts, toolbox) and `BlockSvg.setDragStrategy`. Private members are not used."
     - "Each subclass of a Blockly or plugin class is listed in the developer guide with the upstream version it was checked against."
     - "A patched dependency (pnpm `patchedDependencies`) is allowed only with benchmark or defect evidence, a linked upstream issue or pull request, the smallest diff, and a test that fails without it."
   - Consequences: "Every upgrade of Blockly or its plugins re-checks the listed seams (M3 spike)."
5. **[Owner] New ADR-0015, written before W4-1 starts: "Block upgrades run on load; before 1.0 a retired block type is replaced at once".** It amends ADR-0004. Status: Accepted (D18, decided 2026-10-10). It records:
   - the replacement contract: the ID, position, comment, flags, stack and nested blocks are kept;
   - retired IDs are never reused (`RETIRED_BLOCKS`, validated);
   - upgrades run after the §5.6 limits and before trust is evaluated, never change the security hash, and the result is checked against the limits again;
   - after 1.0, a deprecation period with a badge.
6. **[Owner] New ADR-0016 (M3): "The live preview runs in a Web Worker".** It amends ADR-0010. Status: Accepted (D15, decided 2026-10-10). It records:
   - **What moves:** `update` and `canonical_text` run in the worker.
   - **What stays on the main thread:**
     - scope-index lookups stay synchronous;
     - `clipboard_make` and `paste_prepare` use a second instance on the main thread, or become asynchronous.
   - **The index:** its format, and a test that it answers exactly like `symbols_in_scope` for every block of every example and security file.
   - **Security:**
     - verify on WebKitGTK and WebView2 that the worker script's response carries the CSP (*hypothesis* that it does), and add it if not;
     - no Tauri IPC in the worker;
     - the JSON rules of 05 §5.6 apply.

### 01 Overview

7. **§1.4 G3** *(correction)*: "**G3 Live C++ preview.** Generated C++ follows your edits: it updates within 100 ms of the last change (a 50 ms pause, then analysis and generation within N4), with two-way highlighting between blocks and code."
8. **[Owner] §1.4 N4 and new N12.** N10 (Reliability) and N11 (Determinism) already exist (`docs/spec/01-overview.md:95-96`), so the new goal is N12.
   - N4: "< 50 ms (p95) for the compiler core's `update()` (load, analysis and generation) of a 1,000-block module, measured in the webview."
   - New row: "N12 | Interaction latency | Selecting a block, choosing a toolbox category, opening a dropdown and dropping a block respond within 100 ms (p95) in a 1,000-block module. While a program of up to 200 blocks is edited, no main-thread task exceeds 50 ms. Met in M5; until then the benchmarks of 09 §9.2 report it and gate regressions."

### 02 Architecture

9. **§2.3 tree:**
   - add `BUILDING.md  # build and run from source` and `docs/developer-guide/  # contributors: architecture, security settings, dependencies, testing, recipes`;
   - mark `i18n/  # message catalogs (M5)`;
   - add `e2e/  # test seams for the WebDriver suite (E2E builds only)` and `test/  # Vitest setup and helpers` under `apps/desktop/src`.
10. **§2.5.2 command rows.** These are additive, so `IPC_VERSION` stays.
    - `settings_update`: "`{ codeStyle?, run?, console?, ui? }`; `ui` takes a partial `layout` (05 §5.9)" (with W3-4).
    - `project_open_dialog`, `project_open_recent`, `project_reload`, `recovery_restore`: add `blocksUpgraded`, "true when 05 §5.7 upgraded a block in memory" (with W4-1).
    - `project_set_dirty` note: "Open, new and reload start clean unless `blocksUpgraded` is true; a restore starts dirty."
    - `recent_list`: "`projectName` is the name the project had when it was last opened or saved, including a plain `project_save`."
11. **[Owner] §2.6 UI thread:** "UI thread (webview): Blockly and React. In M2 the WebAssembly preview runs on the main thread. The 1,000-block webview benchmark missed N4 in M2 (about 120 ms p95), so from M3 the compiler core's `update()` runs in a dedicated Web Worker (ADR-0016). Each preview then carries a compact scope index produced by the same analysis, so fields, mutators and the toolbox answer scope questions synchronously. Incremental analysis follows in M5."
12. **§2.8, the Linux cell of the Webview row:** "WebKitGTK 4.1. Known GPU and driver issues are documented with workarounds (e.g. `WEBKIT_DISABLE_DMABUF_RENDERER=1`) in `BUILDING.md` § Troubleshooting and in the user guide."
13. **§2.4 start-up** (only if W6-5's runtime check is adopted): "At start-up the editor also compares the WebAssembly core's catalog version with the backend's (`app_info.catalogVersion`); a mismatch is the same blocking start-up error as an IPC version mismatch."

### 03 Block language

14. **[Owner] §3.1, a new paragraph after "Rule of thumb":**
    - "**One idea, one block, one toolbox entry.** Variants of one idea that share a shape are choices on one block (a dropdown, a ⊕/⊖ part or a checkbox), never separate toolbox entries. A separate block type is used only when the variants differ in shape (a call used as a statement or as a value) or in what the block holds (*forever* has no condition). A toolbox preset may fill in a value, but never stands in for a choice the block offers in place."
    - Also: "In M3, *with arguments* is a checkbox on *when program starts* (a compatible field), not a second block." (D4)
15. **[Owner] §3.3, a "Placing blocks" paragraph after the shapes table (D13 option (a)):** *Superseded by 03 §3.3's text, pending the owner (D13's follow-up): W5-2 shipped "dropped with its top on a statement" instead of "with its mouth at a statement".* "A dragged block snaps to the nearest place that accepts it when its connection is within about 44 screen pixels of that place, and never less than 48 or more than 96 canvas units away. A grey preview shows where it will go. An empty C-block dropped with its mouth at a statement wraps that statement and everything below it in the same list, as in Scratch; an empty C-block is never inserted between two statements. Hats and definitions never wrap. A drop with the pointer over the toolbox or the trash can deletes only the dragged blocks and never connects them: while the pointer is there no place is previewed, and the dragged blocks are shown faded. Wrapping is a pointer gesture; its keyboard equivalent comes in M5."
    - If D13 chooses (b), replace the insertion clause with: "Dropped with its top at the gap between two blocks, it is inserted there instead; when both are in reach, wrapping wins when its distance is at most the insertion distance plus 8 units."
    - If W5-2 moves to M3, mark the wrapping sentence "(M3)".
16. **[Owner] §3.3, a "Parked blocks" paragraph:** "As in Scratch, a statement or value block, or a stack of them, can be left anywhere on the canvas. Only statements attached to *when program starts* or to a function run; definitions are never parked. Parked blocks are saved, show a ⚠ *not attached* badge, are not checked until attached and never stop Run (ADR-0012). Until M3, a variable created inside a parked stack can be chosen in the blocks below it only once the stack is attached."
17. **§3.4 Storage, replacing the paragraph after the JSON example:** "A token is an object with exactly one key. The kinds are `num`, `str`, `chr`, `ref`, `op`, `kw` and `text`; a new kind needs a new format version (05 §5.7). Values are checked when the slot is parsed, not when the file loads: an `op` or `kw` outside this section's grammar, or a `num` that is not an accepted literal, is a slot error (`E0310`, …), and the file still loads. In M2 the accepted forms are the operators `+ - * / % == != < <= > >= && || ! and or not ?:`, parentheses, `true` and `false`, and numbers written as decimal digits (no leading zero except a lone `0`) with an optional fraction and exponent, or as `0x`/`0b` integers, with `'` separators. Suffixes and the other operators (08 §8.4.4) come with typing in slots (M3). A `num` may start with one `-` or `+`, which lowering applies as a unary operator; the literal itself (`NumLit`) never carries a sign. The file keeps numbers as typed; the generated C++ prints them in canonical form (08 §8.4.4)." Also, in the "In M2" list: "a number typed with a leading `-` is stored as one `num` token".
18. **[Owner] §3.6 Symbols and scoping:**
    - (a) The dropdown sentence of "Scope follows C++" (with W1-19, then W4-4): "The dropdowns of blocks that change a variable list only what may be changed there: they leave out constants, a `for` loop's counter inside its loop and read-only parameters, and *change* and the update operators also leave out text and true/false variables. A reference already chosen stays shown. Any other type mismatch is left to the analyser. A parked block's dropdowns list what is visible at the end of `main`'s body; a block inside a disabled statement lists what is visible at that statement."
    - (b) The implicit-globals bullet (with W4-2): "*Make a variable* asks for a name and a type (`int` by default) and inserts a declaration."
    - (c) The end of the "Unlike Scratch" bullet (`03-block-language.md:223-226`).
      - With W1-6: "With nothing selected, or with a block selected that is not attached to `main` or a function, *Make a variable* inserts at the top of `main` (creating `main` if there is none), and the Variables category lists the symbols visible at the end of `main`'s body."
      - Replaced with W4-2 by: "With nothing selected, *Make a variable* inserts at the top of `main` (creating `main` if there is none). The Variables category lists every variable of the module, grouped by the function that declares it, and does not depend on the selection (04 §4.2)."
19. **[Owner] §3.7.2 Variables table:**
    - With W4-2: the `change` row adds "for number and character variables". After the table: "The toolbox does not repeat these blocks per variable: it shows one *set*, one *change* and one update block whose menu chooses the variable (04 §4.2)."
    - Create row: "`create [int ▾] variable [score] = (0) constant ☐` → `int score = 0;` (`const int` when ticked; the ⚙ popover with `constexpr` and `static` replaces the checkbox in M3 as a compatible change)".
    - With W4-5, replace the `change` and `*=` rows with: "`change [score ▾] [by ▾] (1)` → `score += 1;` (`++score;` for *by* the literal 1); the menu also gives *down by* (`-=`), *times* (`*=`), *divided by* (`/=`) and *mod* (`%=`), and, with *Show advanced blocks* (M3), the bitwise forms `&=`, `|=`, `^=`, `<<=` and `>>=`. In C++ label mode every option shows its operator. For number and character variables."
20. **§3.7.4 Text:**
    - `join` row: "… `+ std::to_string(n)` for whole numbers and `+ b2c::to_text(x)` for decimal numbers, which writes the number exactly as `print` would". Add `to_text` to the §3.9 helper list.
    - New row: "`character 'a'` → `'a'` (one ASCII character; use text for anything else)".
21. **§3.7.5:**
    - if row: "One block: *else if* and *else* are added and removed with ⊕/⊖ on the block."
    - Repeat row: "`repeat [while ▾] <c>` (choose *until* in the menu) → `while (c)` / `while (!(c))`".
22. **§3.7.6:**
    - Text ask row: "*keep asking until valid* (default): `name = b2c::ask_line("Name? ");` reads one line; an empty line asks again".
    - Numeric note: "An answer is valid only when the whole line, apart from spaces, is one value of the variable's type: `3.5` or `12abc` for a whole number asks again with *Please enter a whole number.*"
    - Print row label: "print … new line ☑ to [the console ▾]"; *the error stream* writes to `std::cerr`.
23. **§3.7, a new "In M2" paragraph after the intro** *(correction)*: "In M2 the catalog has the blocks of `catalog/core/` only: *create* with a *constant* checkbox (no `constexpr`/`static`), *set*, *change*, the update operators `+= -= *= /= %=`, getters; arithmetic, comparison, *random integer*, *as int/double*; and/or/not, true/false, conditional value; text, character, *join*; *if/else if/else*; *repeat*, *repeat while/until*, *for*, *forever*, *leave loop*, *skip to next round*; *print*, *ask*; *define* (copy, editable and read-only parameters; a read-only number, character or true/false is passed by value), *return*, calls; *when program starts*, *stop program*."
    - In §3.7.7, replace `03-block-language.md:361-362` with: "Recursion is supported. Overloads (same name, different parameters) and default arguments come in M3 (10 §10.1); until then a second function with the same name is `E0211`."
    - The coverage matrix (§3.12) stays as it is.
24. **[Owner] §3.7 Functions row, §3.7.7 and §3.8 item 1:** replace *My Blocks* with "*Your functions*: a call block for each function you define, kept up to date (grouped under a module heading only when the project has more than one module)". The name follows D11.
25. **§3.11.1:**
    - **Toolbox bullet:**
      - "… and, for *Program*, *Variables* and *Functions*, a dynamic kind whose entries the editor fills in (04 §4.2) …".
      - Drop the *repeat until* and *if … else* preset examples, and keep `var.declare` and `io.ask`.
      - "A block has at most one entry (§3.1)."
      - Reachability: "(`var.get`, `var.set`, `var.change` and, until W4-5, `var.update` through Variables; `program.main` through Program; `func.call` and `func.call_stmt` through Functions)".
    - **Validation:** "no block has two entries; no entry label equals a category name (the editor's tests check the same for its dynamic headings); every field and non-repeated value input appears in the friendly label, and a checkbox is next to the words naming it".
    - **Messages,** replacing `03-block-language.md:606-607`: "Messages and block paths use a block's short name: its optional `short` key, or else its friendly label up to the first field or input. They never show a block's catalog ID, except for an unknown block type (`E0601`)."
26. **[Owner] §3.11.3, replacing the section (D18):** "Block IDs are stable and never reused. **Compatible** changes (a field, `extra` key or input with a default; a dropdown option; label, help or colour text) keep `version` and raise `generator.catalog`'s minor version. A file that leaves the new key out loads with the default, and the editor keeps leaving it out while the block keeps that default; blocks a newer editor creates write it. When a document or clipboard payload names a `generator.catalog` newer than this app's, a block with an unknown field, `extra` key or option is reported as made by a newer catalog (`B2C-E0602`, not `E0605`), shown as a placeholder and kept unchanged on save. **Breaking** changes raise `version` and ship a migration (a pure BDM → BDM function). **Merging or retiring a block type** ships a *replacement* from the old type and version to its successor that keeps the block's ID, position, comment, flags, stack and nested blocks, and lists the old ID as retired (ADR-0015). Before 1.0 an exact replacement takes the place of a deprecation period. Migrations and replacements run when a document is loaded, before the editor or compiler see it (05 §5.7), so the editor never shows an upgradable block as a placeholder. Each has a golden-file test."
27. **§3.8 item 1 and §3.9 "Placement"** *(correction)*: "**In M2** every module is compiled on its own: functions cannot be called across modules (`E0206`), their names are unique in the program (`E0211`), and helpers are inline in each file." Add the same note to 06 §6.7.

### 04 User interface

28. **[Owner] §4.1 "In M2" bullets:**
    - **≡ menu:** add *Rename project…* after *Save as…*. With W3-9, add "*Undo* (`Ctrl+Z`) / *Redo* (`Ctrl+Y`) for the block editor, which work wherever the focus is except in text fields, dialogs and a running program's console".
    - **Top bar:** "The top bar shows the project name, followed by `•` while there are unsaved changes. Clicking the name (or *Rename project…*) edits it in place: Enter or moving the focus away keeps it, Escape cancels. A name has 1–100 characters on one line, without control or bidirectional-override characters, and spaces at either end are removed. Renaming is an unsaved change, is saved as `project.name` and never renames the file; *Save as…* suggests the name as the file name."
    - **Layout:** "On first start the window opens at 1280 × 800, made smaller when the screen's work area is smaller. The C++ dock starts at 30 % of the window's width (280–480 px) and the bottom dock at 28 % of its height (120–280 px); each dock takes at most 65 % of the window, and resizing the window keeps these limits. The toolbox's flyout and the docks are resizable by pointer and keyboard, and the docks are collapsible. When less than 200 px of canvas would remain, the flyout narrows first, then the C++ dock is hidden. Swapping and splitting docks, Focus mode and Presentation mode come in M5."
      - With W3-4, add: "The flyout's width, the docks' sizes and whether each dock is hidden are remembered on this computer (05 §5.9)."
    - **New bullet, canvas controls:** "The canvas has *Zoom out*, *Zoom in*, *Reset zoom* (back to 100 % around the centre) and *Show all blocks*, which scrolls to the blocks without changing the zoom. `Ctrl` + wheel or a pinch zooms around the pointer; `Ctrl+=`, `Ctrl+-` and `Ctrl+0` zoom from the keyboard. The controls are named buttons reachable with Tab after the canvas."
    - **New bullet, overlays:** "Menus, hints and dialogs are always drawn above the canvas, the toolbox and dragged blocks; opening a dialog first closes the block editor's drop-downs and field editors."
    - **New bullet, menus:** "A block's menu offers Cut, Copy, Paste, Duplicate, Add comment, Collapse/Expand, Disable/Enable and Delete; the canvas menu offers Undo, Redo, Paste, *Clean up blocks* (lines the top-level blocks up in a column), Collapse/Expand blocks and *Delete N blocks*, which counts the blocks shown and asks with *Cancel* as the default."
    - **New bullet, trash can:** "Dropping a block on the trash can deletes it; the trash can keeps nothing, and Undo brings deleted blocks back."
    - **Sketch:** "zoom − + 100% show all".
29. **[Owner] §4.2 Toolbox:**
    - **Continuous bullet:** "Choosing a category scrolls the flyout to it, and that category stays selected at every zoom level; scrolling by hand selects the category at the top. The flyout shows its blocks at one size whatever the canvas zoom, is as wide as the user drags its edge (default 320 px), is opaque, and cuts off a block wider than itself (which can still be dragged out). Its blocks always show current names. Rebuilding a dynamic category never moves what the flyout shows: the category in view stays in view."
    - **Presets bullet:** "Entries can carry presets that fill in values (a new `int` starting at `0`; *ask* with `"Your answer: "`). A new variable's start value follows its type, and so does the value of *set* when another variable is chosen in it while the value is still a start value. A block whose forms differ only by a dropdown or ⊕ part appears once."
    - **Variables, interim text with W1-6:** "*Variables* lists the variables in scope just after the selected statement (for a selected value block, at its statement). With nothing selected, or with a block selected that is not inside *when program starts* or a function, it lists those visible at the end of `main`; a block inside a disabled statement, or one the analysis has not seen yet, lists the scope of the nearest analysed statement around or before it. The list never empties while the next analysis runs."
    - **Variables, replaced with W4-2 (D2 option B):** "*Variables* offers *Make a variable* (name and type), the `create` block, one `set`, and, when a number or character variable exists, one `change` and one update block, then a getter for each variable of the module, grouped by the function that declares it (at most 50). It does not depend on the selection. A *set* or *change* block dragged from the toolbox comes with a variable already chosen; if that variable is not visible where the block is dropped, the block takes the first fitting variable visible there, as part of the same drop (one Undo removes it). A getter keeps its variable: if the variable is not visible where it is dropped, the block shows `B2C-E0203` and its menu offers the variables that are. A block still in the toolbox lists every variable of the module in its menu, grouped like the category."
      - If W4-2's spike chooses no re-targeting: "A block dragged from the toolbox keeps its variable; if it is not visible there, the block shows `B2C-E0203` and its menu offers the variables that are."
    - **Program (W4-6):** "*Program* offers *when program starts* only while the project has none; otherwise it says where the program starts."
    - **Functions (W1-9):** "*Functions* lists *define* and *return*, then, under *Your functions*, a call block for each function of the module shown, or a one-line hint when there is none; module headings appear only when the project has more than one module. A call block of a known function has as many arguments as the function has parameters."
    - **Rebuilds:** "The flyout is rebuilt only when the catalog, the settings or the module's variables and functions change, never during a drag, while a field is edited or while a category scroll runs."
30. **§4.3 Copy bullet and §4.6:** "*Copy all*, *Copy selection*, the setup page's *Copy* and the link dialog confirm with *Copied* for 3 seconds and announce every copy; *Could not copy* stays until the next attempt. The confirmation never moves the code."
31. **[Owner] §4.4:**
    - New paragraph: "**Parked blocks.** A block left loose on the canvas shows the ⚠ badge of `W0505` and is listed in Problems, but ▶ Run stays enabled; the program runs without it."
    - In "In M2": "*Learn more* is one button in the Problems panel that opens the published diagnostics reference in the web browser (it says so); a link per diagnostic comes in M5."
    - Block paths: "the program block's label (*when program starts*; `main()` with C++ labels, M3)" instead of "`main`".
32. **[Owner] §4.5 Console, replacing the last bullet:**
    - "**Between runs** the console keeps earlier output as scrollback and never writes over it. Every run starts with the terminal's modes reset (the alternate screen left, colours, cursor visibility, scroll region) without moving the cursor, below a dim *── New run ──* line when there is output since the last Clear, and the view scrolls to the end."
    - "**Clear** empties the screen and the scrollback; while a program runs, the line with the cursor (such as a prompt waiting for input) stays."
    - "The console scrolls by itself, never the panel around it, at any display scale and while a program writes; output arriving while the user has scrolled up does not move the view (a new run does). The terminal is fitted to whole rows and columns inside its padding."
    - "Program output is available to screen readers: new output is announced politely in batches, and the scrollback can be read line by line."
    - If D5 chooses (c): "With *Clear the console when a run starts* (Settings) the console is cleared instead, and no separator is written."
33. **§4.6 "In M2":** "While the first discovery runs and no usable toolchain is known yet, ▶ Run stays enabled: the build waits for the discovery (at most 30 s, 07 §7.2) and the build output says so. Only when discovery has finished without a usable g++ does Run open the setup page."
34. **§4.7 "In M2":** "`Ctrl+=` / `Ctrl+-` zoom the canvas around its centre and `Ctrl+0` resets the zoom (D10); Ctrl + wheel and a pinch zoom around the pointer, the plain wheel scrolls. Wrapping statements in a C-block (03 §3.3) is a pointer gesture in M2; its keyboard equivalent comes with the non-drag alternatives in M5."
35. **§4.8 "In M2":**
    - "The app and the blocks follow the system's light or dark setting; the console is dark in both. Controls the webview draws itself are styled explicitly, so their contrast holds in both themes; this is checked in the running app on both systems."
    - "Short confirmations are announced once and disappear after about 3 s; messages that ask for an action stay until the user acts."
    - "The Tab order runs: toolbar, the banners under it, the module switcher (when shown), toolbox, the toolbox's blocks, the *Resize the toolbox* separator, canvas, the canvas's zoom controls, then the docks."
36. **§4.12 and §4.13:**
    - §4.12, later items: "*Appearance*: System, Light or Dark (M3)".
    - §4.13, M3 row: "the Appearance setting".
    - §4.13, M5 row: "swapping and splitting docks" (remembering moves to M2 if W3-4 lands); "tidy-up beyond *Clean up blocks*".

### 05 Project format

37. **§5.3:**
    - "`project.name` is the display name shown in the top bar, the recent list, recovery offers, the trust dialog and the generated header comment. It is independent of the file's name; renaming is an ordinary edit that changes the content hash."
    - Mark `options.usingNamespaceStd` as "(M3; a file that sets it has `formatVersion` 2 or higher)" and `project.lints` as "(M5; `formatVersion` 3 or higher)".
    - *(correction)* Remove `"description": ""` from the example and add "`description` is written only when it is not empty."
38. **[Owner] §5.4 Loose blocks:**
    - First bullet: "A loose block is an ordinary top-level block with `x` and `y`. A loose statement, reporter or predicate, with its stack, is *parked* (ADR-0012): it is saved and shown, but not lowered; it declares nothing, generates nothing and never stops a build. The analyser reports each enabled parked head once as `B2C-W0505`."
    - Replace `05-project-format.md:175-177` with: "A parked stack is reported once, as `W0505` on its enabled head; its stacked blocks get no placement diagnostic. `B2C-E0604` remains for placements the editor cannot produce."
    - `type`/`v` row: "Unknown type → the block is kept and reported as `B2C-E0601` (naming the missing pack); newer `v` → `E0602`; an older `v` or a retired type → upgraded on load (§5.7)."
    - Add ADR-0012 to the related ADRs in the chapter header (`05-project-format.md:3`).
39. **§5.7** (format-version text with W0-3; upgrade text with W4-1):
    - The format-version text of ADR-0013 (item 3).
    - "When a project, recovery snapshot or clipboard payload is loaded, `b2c-catalog` upgrades in memory every block with an older `v` and every block of a retired type, before the editor or compiler sees it. Block upgrades run after the document has passed §5.6 and before trust is evaluated. A migration or replacement never adds blocks, nesting or tokens and never changes a block's ID. It never changes the security summary or the security hash (08 §8.3.1): the text fields of `raw.*` blocks, libraries, packs and defines are carried over unchanged; a change that must alter them bumps the hash's version tag (`b2c-trust-v2`) and asks for trust again. Each upgraded block is checked against §5.6 again, and a violation leaves that block unchanged with `B2C-E0603`; the whole upgraded document is then checked once more (security hash and summary unchanged, §5.6), and a violation there undoes every upgrade of the document and reports each upgraded block as `B2C-E0603`. The project is marked changed; the upgraded form is written only on explicit save, keeping the `.b2c.bak`." (Second check made explicit in the W4a review.)
    - "`b2c migrate` applies format migrations and block upgrades and writes the result (`--in-place` keeps `.b2c.bak`). `b2c fmt` only re-serialises and never upgrades blocks. `b2c check`, `generate` and `build` upgrade in memory only."
40. **[Owner] §5.9:**
    - With W3-4: `"ui": { "layout": { "toolboxWidth", "rightDockWidth", "bottomDockHeight", "rightCollapsed", "bottomCollapsed" } }`, with the ranges 160–1200, 200–960 and 96–720. When a key is absent, the first-start rule of 04 §4.1 applies. "`settings_update` accepts only `codeStyle`, `run`, `console` and `ui`, each partial." M3 adds `ui.theme` (`system`, `light` or `dark`), and `console.clearOnRun` if D5 chooses (c).
    - recent.json: "An entry's `projectName` follows the project: it is updated whenever the project is opened, saved or saved as."
    - trust.json *(correction)*: "Every evaluation reads the file again. Trust is evaluated when a project is opened, reloaded or restored (08 §8.3.1), so a revocation in one instance holds in the others the next time they open the project. Build and Run use the open project's evaluated trust."
41. **§5.10:**
    - *(correction)* "Autosave writes recovery snapshots every 30 s while the project has unsaved changes, and when the window loses focus."
    - *(correction)* "…a check happens at most 2 s after the first event while events keep coming."
    - Atomic save (D16(4)): "A new project file gets the permissions any new file of the user gets (the umask applies); a saved project keeps the permissions it had. Machine-local files and recovery snapshots are always `0600`."
42. **§5.12, paste targets (with W4-4):** "A paste on the canvas, or into or directly after a parked block, has the parked scope (06 §6.5). A block inside a disabled statement has that statement's scope."

### 06 Compiler pipeline

43. **§6.1:**
    - "A stage that finds errors still produces output where it can. The editor's preview, `b2c check` and the build all run stages ①–⑤ on any document that loads, so they report the same diagnostics; only a load failure stops early. Generating and compiling files requires zero errors."
    - With W4-1, in §6.1 and §6.3: "Block upgrades (05 §5.7) are the first step of stage ②, and the app also runs them when it loads a document for the editor."
44. **§6.3, second bullet** *(correction)*: "Fields are validated against their kinds: dropdown values must be one of the options, checkboxes must be booleans, number fields hold non-empty text, and declarations have the `{sym, name}` shape. Whether a number is a valid literal in range (`E0310`, `E0517`) and a name a valid identifier (`E0220`) is checked when lowering."
45. **§6.4 and §6.6:**
    - "**Parked blocks** (top-level statements, reporters and predicates with their stacks, 05 §5.4) are not lowered. The analyser reports `W0505` on each enabled parked head (warning by default; a project can raise it to an error from M5) and records the parked scope."
    - §6.6 points to `docs/reference/diagnostics/` as the normative list of codes.
46. **§6.5 Scope query** (with W4-4):
    - Replace `06-compiler-pipeline.md:143-145` with: "…from the same scope stack the analyser resolves names with, so a dropdown on an attached block never offers a symbol that is out of scope there. A parked block's dropdowns offer the parked scope, the field's filter leaves out symbols the block can never accept, and other type mismatches are left to the analyser (03 §3.6)."
    - Replace the empty-list sentence with: "A parked block (loose, in a parked stack, or nested in one) answers with the parked scope: what is visible at the end of its module's `main` body, or the module's functions alone when there is no `main`; declarations inside parked blocks declare nothing. A block nested in a disabled statement answers like that statement. An unknown block, or one nested too deeply, gives an empty list."
    - With ADR-0016 (M3), append: "From M3 the editor answers scope queries from the scope index each preview carries, produced by the same analysis."
47. **§6.8, §6.9, §6.11, §6.12** *(corrections)*:
    - §6.8.1: fresh names are "`i`, `j`, `k`, `i2`, `i3`, …".
    - §6.8.2: "The emitter's write functions accept only these types or `&'static str` text chosen by the generator. The one function that appends raw text is private to the printer module."
    - §6.8.3: "In M2 only stream statements are broken (each continuation starts with `<<`); the document algebra comes with the code style options (M5)."
    - §6.9 (M3): "The editor receives the source map in a compact form."
    - §6.11: "In M1/M2 `b2c generate --export` only drops the banner; the folder layout comes in M3."
    - §6.12: mark `MessageKey` (M5) and `fixes` (M3); add the `E05xx` and `I05xx` ranges.
48. **[Owner] §6.13:**
    - "In M2" bullet (with P-1): "Each debounced change runs one `update()` in WebAssembly: the document is loaded and hashed once, and the preview and hash are returned; the canonical text is produced on demand (save, build, autosave, recovery), and the dirty flag compares hashes. N4 covers the compiler core; what the user waits for also includes Blockly's rendering, which N12 bounds and the edit benchmark measures."
    - Replace `06-compiler-pipeline.md:507` and `:510-512` with: "In M2 the analysis runs on the main thread. From M3 it runs in a Web Worker for every module size (02 §2.6, ADR-0016); the per-top-level-block cache comes in M5."

### 07 Toolchain, build and run

49. **§7.2** (with W3-10): "**Never searched:** the open projects' folders, each as a tree, except that a project folder which is the user's home folder or a file-system or drive root is excluded as that folder only; the build cache; the process's current directory (that folder only); removable or network (UNC) paths; and relative `PATH` entries. At startup the app makes an empty owner-only folder (`<cache>/cwd/`) its current directory; the CLI keeps the caller's. The same folder rule applies to a rescan, to the build-time check (`B2C-T1002`) and to *Choose g++ manually…*. A compiler a search skips is listed with its reason." Linux (M3): "`g++`, then every `g++-N` with N ≥ 11, newest first."
50. **§7.3:**
    - With W3-12: "A compiler whose probe timed out is probed once more with a 30 s timeout before it is reported unusable (security software can delay the first start of a new program); the setup page says so while that runs."
    - *(correction)* The Debugger row is marked "(M5)".
51. **§7.5** *(correction)*:
    - "LRU eviction when the cache exceeds 2 GiB (configurable from M5). Entries untouched for 30 days are pruned at startup and after each build. *Clear build cache* is on the Settings page."
    - "Timeout per TU: 120 s (a setting from M5)."
    - "(M3) Sandbox folders unused for 30 days are removed at startup unless a program runs in them."
52. **§7.6.1** *(correction)*: "`build_start` validates the document and keeps it as the project's latest document, then checks trust: a restricted project gives `restricted`, and no build folder is created and no process is started."

### 08 Security

53. **§8.3.1** (with W4-1): "Trust is evaluated on the upgraded document, whose security hash equals the file's (05 §5.7)."
54. **§8.5, Binary planting row:** "Discovery never searches the project folder or the cache, skips relative PATH entries, and the app starts by changing its current directory to an empty folder in its cache (07 §7.2). The status bar names the toolchain a build will use, with its full path in its tooltip and accessible name; the toolchain page shows every path."
55. **§8.6** (D16(4)): "A new project file, and its `.b2c.bak`, gets mode `0666 & ~umask`, set on the temporary file before the rename. An existing file keeps its mode. Machine-local files and recovery snapshots are always `0600`."
56. **§8.9:**
    - Updates row: "Dependabot for cargo, npm and github-actions (weekly). Minor and patch updates are grouped per ecosystem, and each major update gets its own pull request. Version updates to new majors of Blockly and its plugins, xterm.js, TypeScript and Vitest are ignored until a roadmap item schedules the migration. An advisory for one of them is still raised by OSV-Scanner and `pnpm audit`, which fail CI, and is fixed by hand under the security-update rule, even when the fix needs the ignored major."
    - New row: "**Patched dependencies** | `patchedDependencies` in pnpm only with an ADR (ADR-0014). Each patch file is reviewed as code (CODEOWNERS), names its upstream issue, is re-applied and re-reviewed on every update of the package, and is removed once upstream ships the fix. Cargo `[patch]` is not used."
57. **§8.12, T10:** "g++.exe in a project folder, the cache or the current directory | Never searched (folder rules of 07 §7.2); absolute canonical paths; fingerprinting".
58. **§8.13** *(correction)*: "Each milestone review is recorded in `docs/security/threat-model-reviews.md` (date, commit, scope, findings, accepted residuals)."

### 09 Quality and delivery

59. **§9.1** *(correction)*:
    - "Known defects, open diagnostics and technical debt are tracked as GitHub issues with a milestone label; status notes link them."
    - Replace the Clippy claim with: "… plus `disallowed_methods` (`std::process::Command::new` outside `b2c-process`). Raw output text is confined by module privacy: the only function that appends raw text is private to the printer module (06 §6.8.2)."
60. **§9.2:**
    - **Table, new row** (with W7-1): "Chrome visual diff | WebDriver screenshots + pixelmatch | The open main menu over the editor, the toolbar drop-downs, the console after two runs, the toolbox after each category, the smallest window; light on both systems, dark on Linux (Windows dark with the Appearance setting, M3)".
    - **Breadth flows** add: "placing blocks with real pointer drags (into a C-block off its connection, a drop on the toolbox while a place is previewed, wrapping statements of `main` once wrapping lands)". The skipped-lines marker is checked on Linux only.
    - **E2E:** "… check the rows on screen where the layout matters (a second run, *Clear*)".
    - **Benchmarks**, replacing the gated-preview definition: "the p95 of the preview pipeline's task at 1,000 blocks (reading the canvas, `update()` in WebAssembly and storing the result)".
    - **Benchmarks**, add:
      - "the edit benchmark includes Blockly's rendering with the C++ panel open; selecting blocks in 8 scopes at 1,000 blocks; and, on the guessing game, the longest task after a selection and after an edit."
      - "Every E2E flow records its longest main-thread task."
      - "A change expected to alter a gated metric runs the A/B benchmark job on its pull request; an intended slowdown is recorded in `e2e/bench/accepted.json` with its reason, which restarts that metric's baseline."
      - "Absolute interaction budgets on small programs are reported in M2 and gated from M3 once the toolbox refreshes per section."
      - "Meeting N2–N4 and N12 at 1,000–5,000 blocks is part of M5."
    - **Mutation testing** (with W4-1): add `upgrade_document` to the list.
61. **§9.3:**
    - *(correction)* "The Windows runs of the breadth flows and the nightly suites pass (scheduled runs since 2026-10-07)."
    - The docs job also runs the all-Markdown anchor check (W0-5) and `check-build-docs.py` (W6-4).
    - (M3) "A weekly `build-docs` job runs `BUILDING.md`'s Debian steps word for word in Debian 12 and 13 containers, up to a release build that starts under xvfb."
62. **[Owner] §9.4:**
    - New rows:
      - "`BUILDING.md` | repository root | everyone who builds from source (all users until M6), contributors | the one place with prerequisites, install commands and build/run steps per system, kept apart from design notes; its Debian and Windows steps are what CI runs";
      - "Developer guide | `docs/developer-guide/` | contributors | architecture, security settings, dependency table, testing, recipes" (with W6-2);
      - "Threat-model reviews | `docs/security/threat-model-reviews.md` | maintainers | one entry per milestone".
    - Desktop README row: "what the app is, its code map and links; no build steps or design notes".
    - Bullets:
      - "Build steps are written once: other documents link to `BUILDING.md`."
      - "Pinned versions appear only in `BUILDING.md` and in their source of truth."
      - "Links and heading anchors are checked for every tracked Markdown file."

### 10 Roadmap

63. **[Owner] §10.1, a new subsection "M2 polish (part of M2)":** "The owner's first trial (Debian, 2026-10) found 18 issues, each tracked as an issue labelled M2-polish. The wave also fixes two data-loss and correctness defects, adds DOM-level chrome checks and small-program benchmarks, and writes `BUILDING.md`. Its items are split into a must-have set and an if-time set. **Exit:** both P0 defects are fixed; each of U1–U18 is fixed or moved to M3 with the owner's agreement; items marked *M2 if time* move to M3 without further sign-off when the wave's budget is spent; the usability protocol and the getting-started guide describe the polished editor."
64. **§10.1 status note:**
    - "M2 is done once the polish wave is finished and these are recorded on a polish build: the threat-model review, the first green weekly run, the toolchain matrix (at least the Debian cell, or a recorded deferral), committed visual baselines, the usability sessions, NVDA/Orca, the demo review."
    - *(correction)* "the visual diff passes" becomes "it runs but compares nothing until its baselines are committed".
    - Readings: "(debug backend, production frontend, Linux under xvfb without a GPU, 2026-10-10): selecting 1.2 s, an edit at the end of `main` 0.85 s, dragging a 1,000-block stack 14 s in all; a toolbox rebuild 0.13–0.17 s at 40 blocks. To be measured again in a release build (W2-2)."
    - *Benchmarked 2026-10-12/13 (W2-1, after P-2 and the review fixes; debug backend, E2E frontend, Linux under xvfb):* the longest task after a click at 1,000 blocks about 1.2 s (`select-1000.max-task`; the toolbox's rebuild about 0.55 s of it), an edit until the page settles with the C++ panel shown about 1.4 s at p95 (`edit-1000.p95`), the preview task about 100 ms at p95; on the guessing game a selection about 140 ms (the toolbox's rebuild about 120 ms) and an edit about 33 ms; a program's 200,000 lines shown in the console in about 0.8 s.
65. **§10.1 M2 bullets:**
    - "Dynamic Program, Variables and Functions categories (standard variable set with scope- and type-filtered menus)";
    - "Parked (loose) blocks: kept, warned, never run (ADR-0012)";
    - "A type-aware connection checker, Scratch-style snapping and wrapping" (the wrapping half moves to M3 if W5-2 does);
    - "Remembered toolbox and dock sizes (`ui.layout`)" (only with W3-4);
    - "Docs: `BUILDING.md` and the getting-started page".
66. **[Owner] §10.1 M3, added:**
    - the preview in a Web Worker (ADR-0016);
    - the small-program benchmarks gated;
    - Blockly 13, continuous-toolbox 13 and xterm.js 6.1 after a spike;
    - the patched Blockly render management (if ADR-0014 and the measurement allow it);
    - lowering parked stacks for the scope query and block types;
    - the Appearance setting (System, Light, Dark);
    - `switch` (with fall-through), `do … while`, `wait`, *with arguments* on *when program starts*, overloads, default arguments, the *moved* mode;
    - type-specific variable operations;
    - the project name in the project settings dialog;
    - every polish item moved past the cut line.
67. **§10.1 M5, M6 and post-1.0:**
    - M5: "Dock layout (swap, split)"; "High Contrast themes (light and dark) and a checked colour-blind-safe palette"; change `10-roadmap.md:168-169` to "performance targets (N2–N5 and N12) met"; "a build is up to date when its generated files are identical (07 §7.5.1)".
    - M6: "Trusted Types enforced on WebView2 when the trial reports no violations for a milestone, or an ADR records why not."
    - Post-1.0 item (`10-roadmap.md:206-208`): "after 1.0, a deprecation period for block types (badge and automatic replacement); before 1.0 an exact replacement retires a type at once (03 §3.11.3, ADR-0015)".
68. **§10.2 Risks:**
    - Blockly-performance row: add the readings above, the CI readings (dragging at 5,000 blocks, p95: 40 ms on Linux, 914 ms on Windows) and the mitigation "gated benchmarks with A/B runs; a patched and upstreamed render management and a linear `getDescendants` (M3, if ADR-0014 accepts the patch); the Worker (M3); per-function views (M5)".
    - New row: "Blockly, its plugins and xterm.js move to new majors faster than the app | High | Medium | separate Dependabot PRs, one scheduled migration per milestone, small subclasses listed under ADR-0014 and covered by E2E and visual tests".
69. **[Owner] §10.3 Product decisions, new rows, each "Decided 2026-10-10":**
    - Q10 "Are loose blocks errors? No: parked, warned, never generated (ADR-0012)";
    - Q11 "Preview off the main thread before M5? Yes, in M3 (ADR-0016)";
    - Q12 "Variables listing independent of the selection" (D2);
    - Q13 "Permissions of new project files: the user's umask" (D16(4)).

### Manual tests, guides and reference

70. **`docs/manual-tests`:**
    - **m2-accessibility:**
      - §6: the ≡ menu can be clicked over its full width above the toolbox; Debug/Release is readable while closed in light and dark (GTK light theme); the canvas zooms from the keyboard;
      - correct the step that relies on the webview's Ctrl++;
      - §4 Console: output is read aloud;
      - §2: the toolbox separator is in the Tab order.
    - **m2-usability:**
      - a release build of a polish-wave commit;
      - reworded hints (the while/until menu, ⊕ else);
      - the category check at three zoom levels;
      - the console check at 125% and 150% scaling;
      - "does scope confuse participants?" as a watch point.
    - **New files:** `m2-toolchains.md`, and a profile record (W2-2).
71. **`docs/user-guide/getting-started.md`:**
    - build from `BUILDING.md` (W1-18);
    - steps 5 and 7 use the loop menu and ⊕ (W1-8);
    - step 5 no longer asks the reader to select the loop to see variables (W4-2);
    - "drag until a grey shadow appears; an empty C-block wraps the blocks you drop it on" (with W5-2);
    - "Blocks left loose are kept but do not run; a variable created in a loose stack can be chosen below it once the stack is attached";
    - the zoom buttons and *Show all blocks*;
    - "⟲ Run again runs below a *New run* line; Clear empties the console";
    - "If the window stays blank (Linux)".
72. **`docs/reference`:**
    - `diagnostics/analyser.md`: a `W0505` entry (three messages; fix: "attach, delete, or disable to park silently");
    - `diagnostics/loader-and-catalog.md`: E0604 narrowed, friendly block names in the examples, E0108 without a version;
    - `cli.md`: `check` lists catalog and analyser problems together and exits 0 with only warnings; `migrate` (with W4-1).

<!-- markdownlint-enable ol-prefix -->

## 6. Decisions for the owner

The structured decisions list has the full options and recommendations. They are grouped by the wave they block.

| Batch | Blocks | Decision | Topic |
|---|---|---|---|
| A | W0, W1 | D16 | Polish-wave structure, budget, cut line, toolchain matrix, theme timing, file permissions |
| A | W0, W1 | D17 | Performance targets (N12, N4, the `update()` export) |
| A | W0, W1 | D18 | File compatibility rules (ADR-0013, catalog version, retiring block types) |
| A | W0, W1 | D4 | Merged Control/Loops form |
| A | W0, W1 | D7 | Documentation layout |
| A | W0, W1 | D11 | Name for *My Blocks* |
| A | W0, W1 | D14 | Blockly default menus and the trash can |
| B | W3 | D5 | Console between runs |
| B | W3 | D8 | Window fit and remembered layout |
| B | W3 | D9 | Flyout scale and width |
| B | W3 | D10 | Canvas controls |
| B | W3 | D12 | Rename details |
| C | W4 | D1 | Loose-block semantics, including the parked-stack limitation |
| C | W4 | D2 | Variables listing model |
| C | W4 | D3 | Variable block set |
| D | W5 | D13 | C-block wrapping rule |
| none | nothing in M2 | D6 | Renderer and patched dependencies (ADR-0014) |
| none | nothing in M2 | D15 | Worker, Blockly 13 timing, budget gating |

**Questions for the owner (information, not decisions).** Answers of 2026-10-10: the owner used a release build (the executable), not the dev server, so U11's lag is not a dev-server effect; the desktop session is Wayland, so W3's window fitting (`preventOverflow`) and U14's wheel handling must be checked on Wayland. The scale factor and the exact U14 glitch are still open.

- Did you start the app with `pnpm desktop:dev` or with a built executable?
- What display scale factor, Wayland or X11, desktop environment, GPU and driver, and WebKitGTK version do you use?
- For U14, which glitch do you see:
  - the view not moving while a program prints;
  - the whole console shifting a few pixels;
  - torn text?

  A short screen recording would settle U14 and U11.

## 7. Risks and verification

| Risk | Mitigation | Verified by |
|---|---|---|
| The wave is larger than its budget | Must-have/if-time split, the cut-line rule, two streams, the critical path tracked weekly | The effort table reviewed weekly against closed issues; the owner signs off any must-have slip |
| A larger snap radius and wrap previews make drags slower (insertion markers are about 95% of drag time) | W2-1 before W5; the A/B job on W5-1 | `drag-over-5000` and `drag-stack-1000` frame p95 (if time), A/B medians at 10%. Not built in M2: `drag-5000` never previews, and W5-1's one churn reading (+15 % at 52 % zoom) is open (W5-1's status) |
| The benchmark gate misses regressions at PR time, or blocks intended slowdowns | The A/B job; `accepted.json`; the merge rule for W4-2, W5-1, W3-11 | `bench-compare.py --self-test` for accept and A/B |
| Guessed absolute budgets are red from day one | Report-only in M2; budgets set from measured baselines after W4-2; the section-level refresh spike | W2-2 profile; a recorded post-W4-2 baseline |
| Geometry changes (W3-1, W3-3) break CI flows and baselines | The window-size seam; a recorded re-baseline; baselines committed only in W7-2 | E2E at three sizes on both systems; the C++ dock shown at the Windows runner's size |
| Re-targeting swaps a variable the user expected to keep | Only presets the user did not choose; getters keep their reference; synchronous, in one undo group; spike against "no re-targeting" | `drag.test.ts` cases (single undo, no second preview); the usability sessions |
| A selection-independent listing offers out-of-scope getters | E0203 explains; menus offer the visible variables | `contents.test.ts`; the usability sessions (does scope confuse participants?) |
| Parked-stack limitation confuses users (U5-d) | Documented in ADR-0012, 03 §3.3 and getting-started; the W0505 badge; M3-4 | Usability observation; M3-4 tests |
| Wrapping is unpredictable at Zelos geometry | The W5-2 spike measures swaps; D13(a); fallback to W5-1 only | Spike numbers at ±60 units and three zoom levels; exact-alignment and ±10-unit tests |
| A custom previewer disposes user blocks or leaves the canvas changed after a cancelled preview | Never call the base `hideInsertionMarker` with user blocks inside the marker | Real-pointer `drag.test.ts`; a revert-drag property test; Escape test; placement E2E on both systems |
| Parked blocks hide real mistakes (ADR-0011's concern) | Visible W0505; lint levels in M5; catalog errors still block | A property test that parked stacks never change the C++; round-trip tests that the editor never detaches a stack by itself |
| W4-1 corrupts files, or changes trust, during upgrade | Pure replacements; limits checked again; the security hash never changes; `.b2c.bak`; past the cut line, so not right before the sessions | Golden migration files; idempotence; fuzz target and mutation list; trust-hash equality test |
| A new field (W4-5) opens badly in an older build | Catalog version comparison (E0602) lands in W4-1, before W4-5 | A test with a `catalog` 1.2.0 file opened by the 1.1.0 catalog |
| `screenReaderMode` slows fast output | Batching exists; fallback to a throttled `role=log` mirror | Flood E2E and the long-task recorder; NVDA/Orca pass |
| Overriding Blockly's protected methods and the plugin's internals breaks on the M3 upgrade | ADR-0014 seam list; small subclasses; upstream reports for U17 and the drop-delete bug | The M3 spike branch runs every suite and the chrome checks |
| What the owner sees differs in a release build (dev server, StrictMode) | `BUILDING.md` in W1; W2-2 | The owner re-checks in a release build on Debian |
| The fractional-scaling wheel bug (U14b) is not the owner's actual glitch | Ask for a recording; the WebKitGTK compositing workaround is documented | Manual test at 125% and 150%; WheelEvent unit tests |
| `preventOverflow` finds no primary monitor on Wayland (near-certain: GTK 3.24's Wayland backend has none, W2-2) | A fallback from the window's current monitor in W3-3; checked on the owner's session in W2-2; the window still opens at 1280×800 | Manual check; the profile record |
| Dependabot's `ignore` also suppresses security pull requests (*hypothesis*) | OSV-Scanner and `pnpm audit` still fail CI; the 08 §8.9 wording covers manual fixes | Read GitHub's documentation in W0-6 |
| The Worker's CSP is not enforced (M3, *hypothesis*) | ADR-0016 requires verification on both webviews | A test page in the M3 spike |
| The benchmark history is lost when the default branch changes (RD-8e) | Move the history before 1.0 | Noted in W7-2; checked at the M6 release preparation |
| Chrome visual tests are flaky across fonts and themes | Per-OS baselines; reviewed candidates; DOM assertions (`elementFromPoint`, contrast crop) for the critical cases | Baselines committed by hand (W7-2); a throwaway colour change must fail |
| Spec and code drift again | Spec revisions land in the same PR as the code; issues linked from 10 §10.1; corrections in W0-8 | Docs CI (anchor and build-docs checks); the milestone review |

**Per-wave acceptance:**

- **W0:**
  - ADR-0012/0013/0014 merged as Accepted;
  - the anchor check passes on HEAD and fails on a renamed heading;
  - one green minor/patch Dependabot PR per ecosystem.
- **W1:** every code fix has a test that fails on HEAD, including:
  - the drop-delete pointer test, with a release that has no final move;
  - the `-1` literal;
  - the stacking rule and the `elementFromPoint` E2E (U1);
  - the toolbar contrast crop (U15);
  - the xterm buffer test (U4);
  - the CSS audit (U14);
  - fake timers (U13);
  - the scroll table test (U17);
  - the loose-block menu test (U2);
  - the ask fixtures (L4);
  - `BUILDING.md` followed word for word in Debian 12 and 13 containers.
- **W2:**
  - the A/B job runs on a labelled PR;
  - `accepted.json` restarts a metric's history;
  - the select and edit metrics report on both systems.
- **W3:**
  - E2E at 800×560, 1024×700 and 1280×800 through the window-size seam, each leaving at least 200 px of canvas;
  - *Show all* keeps the scale;
  - rename, save and reopen;
  - restart keeps the sizes, if W3-4 landed.
- **W4:**
  - the guessing-game exit E2E is re-recorded;
  - the catalog coverage test passes, with every block reachable once;
  - the CLI and the app give the same problem codes over the examples and the security corpus;
  - flyout `show()` count is 0 on a selection change;
  - an A/B run before and after W4-2 is attached.
- **W5:**
  - placement E2E on both systems;
  - the 44 px tolerance tuned in the usability session.
- **M2 close:** the owner re-checks U1–U18 on Debian (and Windows) in a release build before the usability sessions.

## Appendix A. Decisions in full

### D1. What happens to blocks left loose on the canvas (U5)? This reverses part of ADR-0011, which you confirmed on 2026-10-05

**Options.**

(a) Parked (ADR-0012). A loose statement or value block, with its stack, is saved and shown and gets one warning B2C-W0505 per stack. It is never generated and never blocks Run. A block you disabled gets no warning. Definitions are never parked, and damaged or unknown blocks (every catalog error except placement: E0601-E0603 and E0605-E0614) still stop the build. (b) The same, but with severity 'info'. (c) Keep error E0604 but let Run go ahead; this splits the CLI from the app. (d) Keep ADR-0011 as it is. Sub-choices: (1) Should parked blocks also look dimmed or dashed? (2) Should a second 'when program starts' be parked instead of being error E0406? (3) In M2, parked code is not analysed. A variable created inside a parked stack is not offered by the menus of the blocks below it, and is not listed in Variables, until the stack is attached. Parked value blocks also accept any value in their slots. Accept this until M3, or lower parked stacks for the scope query now (about 1-2 weeks, on the critical path)?

**Recommendation.** (a), with W0505 as a warning, so a project can raise it to an error once lint levels arrive in M5. No dimming: the badge is enough, and Scratch shows nothing. Keep E0406 for a second 'when program starts', because it would be unclear which one runs. Accept the parked-stack limitation for M2, document it in ADR-0012, the spec and the getting-started guide, and lower parked stacks in M3 (M3-4).

### D2. Does the Variables category depend on the selected block (U2, U3, U11, U17)?

**Options.**

(A) Keep the spec's rule (variables in scope at the selected block) with fallbacks. A loose block, or no selection, lists the end of main. A selected statement lists what is visible after it. The list never empties while analysis runs. The flyout still rebuilds when the selection's scope changes, which costs 130-165 ms per click today. (B) The listing does not depend on the selection. It shows the standard set plus one getter per variable of the whole module, grouped by declaring function and capped at 50. It is rebuilt only when variables or functions change. This can list more getters than today's in-scope list, which is closer to your 'for every variable in the program'. A getter dropped where its variable is not visible keeps it and shows error E0203. A set or change preset is moved to the first fitting visible variable, as part of the same drop. Sub-choice for (B): collapse the groups of functions other than the one being edited. (C) No per-variable getters at all: one generic getter with a menu.

**Recommendation.** (B). It removes the 'options go away' symptom, the largest measured lag in small programs, and the category shifts behind U17, all at once. Do not collapse groups in M2, because that would tie the listing to the selection again; check in the usability sessions whether the module-wide list confuses participants. Ship the small fallback from (A) first (W1-6), since loose blocks need it anyway. Option (C) hides names that beginners need to see.

### D3. What is the standard set of variable blocks (U3)?

**Options.**

(1) Merge var.update into var.change as one 'change [x] [by] (1)' block whose menu offers by / down by / times / divided by / mod. The merge needs the new block-migration mechanism (W4-1, about 1-2 weeks) and touches the file format. Do it in M2 only if the wave's budget allows, or in M3 together with the catalog migration work. In both cases the M2 toolbox shows change and update once each, instead of once per variable. (2) Should 'Make a variable' ask for the type in M2, or only in M3? (3) Should true/false variables appear in change's menu? (4) Group getters by declaring function, by type, or not at all? (5) Wording of the menu options versus the C++ operators.

**Recommendation.** (1) Design the merge now, but place W4-1 and W4-5 past the cut line: M2 if time, otherwise M3. The variables redesign (W4-2) does not depend on them. (2) Ask for the type in M2. (3) No: change lists only int, double and char variables. (4) Group by declaring function. (5) Use the friendly words in friendly label mode and the C++ operators in C++ mode (M3). Type-specific operations ('add to the end of' for text, 'flip' for true/false) and the bitwise forms come in M3.

### D4. What form do the merged Control and Loops blocks take (U7)?

**Options.**

(a) One 'if' with inline add/remove buttons for else if and else, and one 'repeat [while]' with a while/until dropdown, each listed once in the toolbox. The blocks already work this way; only the duplicate toolbox entries go. (b) Blockly's gear mini-workspace mutator for if. (c) Also fold 'forever' into the loop dropdown. Sub-choices: (1) Should the loop default to 'while' (the C++ idiom) or 'until' (Scratch)? (2) Should the default condition 'true' become an empty slot? (3) In M3, should 'when program starts with arguments' be a checkbox on 'when program starts' rather than a second block?

**Recommendation.** (a). This is the spec's inline design and needs no migration. Keep 'forever' as its own block: it has no condition. Default to 'while', as the catalog does today. Keep the 'true' default in M2, because an empty slot would show an error (E0609) on every fresh loop; revisit when typed slots arrive in M3. Yes to the checkbox for 'with arguments' in M3.

### D5. What does the console show between runs (U4)?

**Options.**

(a) Keep earlier output above a dim 'New run' line. The overwrite bug is fixed with one escape code, the view scrolls to the new output, and Clear empties the console completely when no program runs. (b) Clear the screen and scrollback at every run. (c) (a) by default, plus a setting 'Clear the console when a run starts'. Optional: the separator also says how the previous run ended.

**Recommendation.** (a) now. With the bug fixed, the old lines below your output disappear. Add the setting of (c) in M3 if you still want a clean console (future list F-5).

### D6. Should the block renderer change, and may Blockly be extended beyond its documented plugin APIs or patched?

**Options.**

(a) Keep Zelos, Blockly's Scratch-like renderer, and fix hit targets with a zoom-aware snap radius. (b) Switch to Geras or Thrasos: smaller blocks, which are harder to hit. (c) A custom Zelos variant with taller empty C-block mouths: this changes only the look, not the hit test. Separately: ADR-0014 would supersede ADR-0002's 'supported plugin APIs only' clause. It would allow public and protected methods of documented classes, registry replacements, and drag strategies, and it would allow a reviewed pnpm patch of Blockly only with benchmark evidence, an upstream issue, the smallest diff and a failing test. Today an edit at the end of a 1,000-block list costs about 0.85 s in Blockly's render management.

**Recommendation.** (a): keep Zelos. Accept ADR-0014, because the drop-delete fix, the flyout width and wrapping need protected seams. Run the render-management patch as a spike (P-3) and land it in M3 only if the measured gain justifies it and the patch review passes.

**Follow-up (P-3, 2026-10-15).** The spike's measurements justify the patch (P-3's status has them all). Appending to a 500-statement chain draws 4 blocks instead of 503. An edit at the end of the 1,000-block program spends 20 ms drawing instead of 440 ms, and in a local A/B `edit-1000.p95` fell from 1,627.5 to 1,071 ms (−34 %); no other metric moved beyond noise. M3-2 therefore lands it under ADR-0014's rules:

- the bundle patch ([`upstream/blockly-render-management.bundle.patch`](upstream/blockly-render-management.bundle.patch)) as a pnpm `patchedDependencies` entry for `blockly@12.5.1`; `blockly.mjs` and the Node entry both load `blockly_compressed.js`, so the one file covers the app and the tests;
- the upstream issue filed from [`upstream/blockly-render-management.md`](upstream/blockly-render-management.md), with the source patch beside it, and linked from the patch entry;
- the spike's two tests committed with it: the render count, which fails without the patch (503 blocks drawn against at most 4), and the placement property test with Zelos and Geras; both are kept, with the tools that patched and probed the bundle, in [`upstream/p3-spike/`](upstream/p3-spike/README.md), which says how to run them;
- if M3-3 moves to Blockly 13 first, the patch is made again against 13's `render_management.ts` and measured again.

Zelos stays: the patch changes no block's shape.

### D7. How should the documentation be laid out (U18)?

**Options.**

(a) A root BUILDING.md is the only place for build and run steps: Debian/Ubuntu and Windows exactly as CI runs them, Fedora and Arch marked untested. The desktop README is cut to a code map. Design, security, dependencies, testing and recipes move to docs/developer-guide/. The README contains no commands. (b) The same, but the README keeps a short quick-start block, checked by CI. (c) docs/build.md instead of a root file. Folder name: docs/developer-guide/ or docs/dev/.

**Recommendation.** (a) with docs/developer-guide/. BUILDING.md lands early (W1), so you can rebuild and re-check in a release build; the README split follows if time allows. Add CI checks of anchors and pinned versions so the commands cannot go stale. Recommend the release build (tauri build --no-bundle) for using and judging the app, and the dev server only for changing it.

### D8. How is the window fitted to the screen, and should the toolbox and dock sizes be remembered in M2 (U6)?

**Options.**

Window: (i) Tauri's built-in preventOverflow, which keeps 1280x800 where it fits and clamps it to the screen's work area otherwise. (ii) Custom code that sizes the window to 90% of the work area and maximizes on small screens. Remembering sizes: (a) in M2, in settings.json as a validated 'ui.layout' section (this moves 'remembered docks' from M5 to M2 and widens the IPC isolation allowlist); (b) in webview localStorage: no backend change, but outside the validated machine files and lost when WebKit data is cleared; (c) a separate ui-state.json; (d) resizing only in M2, remembering stays in M5. Window size and position: remember now, or in M5.

**Recommendation.** (i) preventOverflow: no new monitor code. For remembering, (a), scheduled as 'M2 if time' (W3-4); if the budget runs out, it moves to M3 without further sign-off. Window geometry is remembered in M5.

### D9. How large are the toolbox's blocks, and how wide is the toolbox by default (U6)?

**Options.**

Block scale: a fixed 0.75 (prototyped at runtime), 0.8, or a later S/M/L setting. Default width: min(natural width, 320 px), or 300 px. Blocks wider than the toolbox: cut off at its edge, as in Scratch, or drawn smaller down to 0.6.

**Recommendation.** A fixed 0.75 scale, and a default of min(natural width, 320 px), adjustable between 160 px and whatever leaves 200 px of canvas. Cut off wider blocks; they can still be dragged out. Shorten the widest preset (ask ... keep asking until valid) later if needed.

### D10. How should the canvas zoom controls behave (U12)?

**Options.**

Replace Blockly's SVG controls with named HTML buttons: Zoom out, Zoom in, Reset zoom, Show all blocks. 'Show all blocks': (a) never changes the zoom; it centres the blocks when they fit, otherwise it goes to their top-left corner; (b) zooms out only as far as needed. 'Reset zoom' goes to 100% or to the 90% start scale. Ctrl+wheel/pinch zoom and the Ctrl+= / Ctrl+- / Ctrl+0 shortcuts: now or in M5.

**Recommendation.** (a): never change the zoom, as you asked. Reset goes to 100%. Turn on Ctrl+wheel, pinch and the keyboard shortcuts now; keyboard users currently cannot zoom at all.

### D11. What replaces the heading 'My Blocks' (U9)?

**Options.**

'Your functions', 'Call your functions', or keep 'My Blocks' with a tooltip. Second choice: should call blocks keep their argument add/remove buttons? These can make the argument count disagree with the function's definition. Optional: a 'run without using the result' form for functions that return a value.

**Recommendation.** 'Your functions'. Show a one-line hint when there are no functions yet, and show module headings only when the project has more than one module. Hide the add/remove buttons on calls to known functions. Decide the run-without-result form in M3.

### D12. Details of renaming the project (U10)

**Options.**

(1) Is a rename part of Ctrl+Z undo? Blockly's undo stack covers only the canvas, so this would need a custom event. (2) Should New project also ask for a name? (3) Should the first Save as rename a project that still has its template name, after the file? (4) Should the name be limited to 100 characters?

**Recommendation.** (1) Not undoable in M2: Escape cancels while editing, and the project can be renamed again; document this. (2) No; clicking the title is enough. (3) No; the name and the file stay independent. (4) Yes: 100 characters on one line, checked with the core's text rules. A plain Save also updates the name in the recent list.

### D13. How should C-blocks wrap existing statements (U8)?

**Options.**

(a) Scratch rule, which matches your words ('if they're hovered over another set of blocks, it should try to encase them'). An empty C-block dropped with its mouth at any statement wraps that statement and everything below it: a loose stack top, the first statement of a list, or one in the middle. An empty C-block is never inserted between two statements. (b) The same wrapping, but an empty C-block can still be inserted between two blocks when its top lines up with that gap. With Zelos geometry the two targets are then almost equally close, so (b) needs a tie-break such as 'wrap wins within 8 units'. (c) Wrap only the single target block. Separately: (1) a keyboard 'wrap' target in M2, or pointer-only until M5? (2) When dropping over the toolbox, does the delete win or the connection?

**Recommendation.** (a): it is what you described and avoids an unpredictable tie. Wrapping comes after a 2-day spike that measures predictability in both webviews. If the spike fails, M2 ships the larger snap radius only and wrapping moves to M3. Wrapping is pointer-only in M2. Over the toolbox or trash the delete wins: no place is previewed, and only the dragged blocks are deleted.

**Follow-up (W5-2, 2026-10-14; recorded 2026-10-15). Pending your approval.** The spike measured (a) as written above, with the C-block's mouth as the handle (its first mouth against the statement's top), and that rule failed the plan's go/no-go: its own preview moves the statements into the mouth, about 50 units from where they were, so the next pointer move is measured against the moved statement. At Blockly's radius its preview flickered between a place and none in 439–451 of 1,250 drops (18–39 with the top as the handle), and the two approaches dropped differently at 91–100 of 625 points (top: 9–12; W5-2's status has the table). The plan said to ship W5-1 only in that case; instead W5-2 moved the handle to the C-block's top (its top against the connection above the statement, the place an insertion would use, as Scratch 3 does), which passed, and M2 ships that (03 §3.3: "dropped with its top on a statement").

- **What you see.** The result of a wrap is (a)'s: the statement and everything below it in its list go into the mouth, and an empty C-block is never inserted between two statements. What changes is which statement is wrapped for a given position: the mouth sits one top row (48–56 units) below the C-block's top, so with the mouth placed on a middle statement S, the statement above S is wrapped. The preview always shows what the drop does.
- **What is open.** The go decision was met only by the changed rule, so it needs your approval. Until then §5 item 15's wrapping sentence is superseded by 03 §3.3's text, and the roadmap marks wrapping done pending this decision. If you reject the top handle, the plan's fallback applies: the wrapping sentence of 03 §3.3 gets "(M3)", and the wrapping half of the 10 §10.1 M2 bullet moves to M3.

### D14. What happens to Blockly's built-in menu items and the trash can?

**Options.**

Clean up Blocks: keep it in M2 (it only stacks top-level blocks, and positions are saved), or hide it until M5's tidy-up. Inline/External Inputs: remove it (it is never saved), or persist it (a format change). Delete N Blocks: replace it with an item that counts visible blocks and asks with Cancel as the default. Trash can: keep it only as a place to drop blocks (it stores nothing, and Undo brings blocks back), or keep restoring from it and empty it per project.

**Recommendation.** Keep Clean up Blocks and amend spec 04 section 4.13. Remove Inline/External Inputs. Replace Delete N Blocks as described. Make the trash can a drop target that stores nothing (maxTrashcanContents 0): today it carries blocks from one project into the next.

### D15. When should the performance architecture and library upgrades happen, and when do budgets block merges?

**Options.**

(1) Move the preview to a Web Worker in M3 (ADR-0016; the spec's own trigger has fired: about 120 ms against the 50 ms target), or wait for M5. (2) Upgrade to Blockly 13, continuous-toolbox 13 and xterm.js 6.1: a spike now and adoption in M3, or stay on Blockly 12 until 1.0. (3) Budgets: gate the small-program budgets (no task over 50 ms, select within 50 ms) in M2, or report them in M2 and gate only regressions (through an A/B job and an acceptance file), with absolute budgets gated from M3.

**Recommendation.** (1) Worker in M3, after the single update() export (P-1). The worker's CSP behaviour must be verified on both webviews first. (2) Spike, then adopt in M3. Use xterm 6.1 once a stable release exists (6.0.0 cannot load under the app's frozen prototype); keep the wheel workaround on 5.5 until then. (3) Report absolute budgets in M2 and gate regressions only. Gate absolute small-program budgets from M3, set from a measured baseline once the toolbox refreshes per section. Guessed 50 ms budgets would fail from day one: toolbox rebuilds measure 130-194 ms.

**Follow-up (P-3, 2026-10-15).** The render patch alone does not bring 1,000 blocks near N12 (P-3's status has the readings):

- **Edits.** With the patch, an edit's longest task is still about 0.73 s (from 1.11 s) and `edit-1000.p95` about 1.07 s (from 1.63 s). In Blockly's render pass, `resizeContents` takes about 0.2 s whether 4 or 382 blocks were drawn. About 0.45 s of the task lies outside the render pass: the insertion itself and the webview's style, layout and paint of the canvas, not measured apart.
- **Clicks.** A click draws nothing, so the patch leaves `select-1000.max-task` (about 1 s) as it is. The cost is `BlockSvg.bringToFront`, called by `Gesture.setTargetBlock`: it re-appends the clicked statement's SVG group, which nests every block below it in its stack (0.3–0.9 s for half of the targets), and `focusNode` grows along the stack (21–181 ms). Neither is render management, and the Worker does not touch them.

The recommendation stands. In M3 the Worker (M3-1) takes the preview's 110 ms off the main thread and the render patch (M3-2) about 0.4 s of drawing per edit; the click and `resizeContents` need a spike of their own, with an upstream report and, under ADR-0014, a patch only on the same evidence. Absolute budgets stay report-only in M2.

### D16. How is the remaining M2 work structured, sized and sequenced?

**Options.**

(1) Fix your observations in a polish wave inside M2, before the usability sessions, or close M2 now and move the fixes to M3. (2) Budget: the must-have set is about 52-116 person-days (about 84 at midpoints); the if-time set is about 33-72. Options: 8 weeks elapsed with two parallel streams, a longer single-developer wave, or a smaller must-have set. (3) The cut-line rule: when the budget is spent, if-time items move to M3 without further sign-off. (4) The toolchain-matrix manual pass: your Debian g++ in M2 and the rest in M2 if time, or all deferred to M3. (5) Themes: an Appearance setting (System/Light/Dark) in M3 after a token refactor, or with High Contrast in M5. (6) New project files: owner-only (0600, as now), or the user's umask (usually 0644, readable in shared folders).

**Recommendation.** (1) A polish wave inside M2: session participants must be first-time users, so do not spend them on known issues. (2) 8 weeks elapsed with two streams (chrome/console/docs and language/editor model), which covers the must-have set at its midpoint estimate. (3) Accept the cut-line rule; a late must-have item comes back to you. (4) Your Debian cell is must-have; the rest is if-time. (5) Appearance setting in M3 (F-1); for now, test the dark theme on Linux with GTK_THEME. (6) Use the umask for project files and keep 0600 for machine files; record it as Q13 in spec 10 section 10.3.

### D17. What are the performance targets, and how are they named in the spec?

**Options.**

(1) A new goal for interaction latency: selecting, choosing a category, opening a dropdown and dropping a block within 100 ms p95 at 1,000 blocks, and no main-thread task over 50 ms while editing up to 200 blocks. It needs a new ID (N12), because N10 is already Reliability. Target: M5, or earlier. (2) Redefine N4 as the compiler core's update() (load, analysis and generation) at 1,000 blocks, measured in the webview, with the C++ panel requirement moved to the edit benchmark. Or keep N4 as written. (3) The single update() export (one load and hash per edit, about 30 ms saved at 1,000 blocks): in M2 if time, or first in M3.

**Recommendation.** (1) Add N12 as the 1.0 target, met in M5; until then the benchmarks report it and gate regressions. (2) Redefine N4 around update(), and put 'with the C++ panel open' on the edit benchmark. (3) M2 if time, otherwise the first item of M3, because the Worker builds on it.

### D18. What compatibility rules apply to project files as the block set changes?

**Options.**

(1) formatVersion per file (ADR-0013): a file gets the lowest version whose keys cover it. Version 2 adds usingNamespaceStd (M3), version 3 adds lints (M5). An unknown key from a newer app says 'made with a newer version' (E0108) instead of 'remove this key'. The alternative is to bump formatVersion for every file whenever a feature lands, which breaks 'files that do not use it are unchanged'. (2) New fields on existing blocks: (a) treat every new field as breaking (a version bump and a migration; old files are marked changed on open), or (b) keep the version and make the catalog version count, so an older build shows a block from a newer catalog as 'made by a newer catalog' (E0602) and keeps it unchanged. (3) Before 1.0, may a block type be retired at once by an exact replacement on load (ADR-0015: same ID, position, comment, flags, stack and nested blocks kept), or are core block IDs frozen from M2 on?

**Recommendation.** (1) Adopt ADR-0013. (2) Option (b). (3) Yes: before 1.0 an exact replacement retires a type at once, and retired IDs are never reused. After 1.0 a deprecation period with a badge applies. This is what merging var.update into var.change (W4-5) needs, whenever it lands.
