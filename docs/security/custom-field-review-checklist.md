# Review checklist: custom Blockly fields, tooltips and block rendering

Project files are untrusted ([spec §8.2](../spec/08-security.md#82-assets-actors-and-trust-boundaries)),
and everything in them can reach the editor: names, text literals, comments,
block types, expression tokens. The editor runs in a webview next to the IPC
bridge, so markup or script that reaches the page from a project is a direct
path to the backend. [Spec §8.8](../spec/08-security.md#88-webview-and-ipc-hardening)
therefore requires that user content is always rendered as **text**, and that
custom Blockly fields and tooltips go through this checklist.

Use it when a pull request adds or changes anything in `packages/blockly-ext`
that shows text, builds DOM or SVG, opens an editor or menu, or reads block
state: fields, field editors, tooltips, block labels, placeholder blocks,
expression shadows, mutator parts, context menus, and the CSS they use. The
pull request template links here.

## Rendering

- [ ] User text is shown only as text: SVG text nodes (a Blockly field's own
  text element, set through `getText_()` / `getDisplayText_()`) or DOM text
  (`textContent`, `document.createTextNode`). Nothing is parsed as HTML or SVG.
- [ ] No `innerHTML`, `outerHTML`, `insertAdjacentHTML`, `document.write`,
  `DOMParser`, `Range.createContextualFragment`, `srcdoc`, or React's
  `dangerouslySetInnerHTML`, anywhere, for any text (including text that
  "cannot" contain markup).
- [ ] No HTML from project content: dropdown options are `[string, string]`
  pairs. An option label is never an `HTMLElement` or an image description
  built from project text.
- [ ] No attribute, URL, CSS class, element ID or style is built from project
  text. Project text is never used as a Blockly block type (Blockly turns type
  names into CSS classes): blocks from a missing pack all use the one internal
  placeholder type and keep their original type as data.
- [ ] CSS is static text written in the source, never assembled from project
  or catalog data.

## Hidden text

- [ ] Every display of user text goes through `visibleInvisibles` (fields,
  tooltips, menu labels, badges), so format characters, bidi controls and
  other invisible characters show as `⟨U+200B⟩`-style placeholders
  ([spec §8.4.6](../spec/08-security.md#846-raw-c-and-hidden-text-defences)).
  One-line displays also show tab and newline.
- [ ] Displays of long text are shortened (`truncateForDisplay` or the
  field's `maxDisplayLength`); the full value stays available (tooltip or
  editor) and the stored value is never cut.
- [ ] The stored value is never changed by display code.

## Tooltips

- [ ] A tooltip is plain text (a string, or a function returning a string),
  built from catalog help or from user text passed through
  `visibleInvisibles`. Blockly renders tooltips with text nodes; keep it that
  way (no custom tooltip renderer that builds markup).
- [ ] No message interpolation of project text: catalog and user text never go
  through Blockly's `%1` / `%{BKY_…}` message handling. Blocks are built in
  code with `FieldLabel` objects, not from Blockly JSON `message0` strings,
  and `fromJson` of our fields does not call `replaceMessageReferences`.

## Values and editors

- [ ] A value set by code (loading, undo, paste, Blockly's own serialisation)
  follows the project text rules (`sanitizeFieldText`: no NUL, no C0 controls
  other than tab and newline, no bidi controls, no lone surrogates, at most
  64 KiB of UTF-8), so the editor can never produce a document the loader
  rejects. Every value the loader accepts round-trips unchanged.
- [ ] Typed text also passes the field's entry rule (names: ASCII letters,
  digits and `_`, at most 64; numbers: literal characters only; `text.char`:
  one character). A refused entry keeps the previous value.
- [ ] `saveState` / `loadState` and `saveExtraState` / `loadExtraState` check
  the shape of what they load and ignore anything else: Blockly state can come
  from a paste. Untrusted objects are never merged into other objects or used
  as prototypes; registries are checked with `Object.hasOwn`.
- [ ] Editors use Blockly's widget div or drop-down div with elements created
  through the DOM API: an `<input>`, or Blockly's menu. No `contenteditable`.
- [ ] Every editor has an accessible name (`aria-label` on the input, on the
  menu), and the field-editor axe test covers it.
- [ ] Calls into editor services (symbols, types, dialogs) cannot break a field:
  they are guarded, and a failing service counts as one with no answer.

## Code and dependencies

- [ ] No `eval`, `new Function`, or string arguments to `setTimeout` /
  `setInterval`.
- [ ] ESLint passes with no new `eslint-disable` for the security rules
  (`no-unsanitized`, `no-restricted-properties`, `no-eval`, `no-new-func`,
  `no-implied-eval`). A disable in this area needs a security review in the
  pull request.
- [ ] No new dependency without the justification the pull request template
  asks for ([spec §8.9](../spec/08-security.md#89-supply-chain)); a Blockly
  plugin counts as a dependency.

## Tests

- [ ] Each field has tests for: its value round trip (directly and through
  Blockly's serialisation), refusal of each forbidden character class and of
  over-long text, and its entry rule.
- [ ] Hostile text (`<b onmouseover=…>`, `<img src=x onerror=…>`, invisible and
  bidi characters) is shown literally: a test checks that no element is created
  from it and that the placeholders appear.
- [ ] axe-core finds no violation in the field's editor or menu.
