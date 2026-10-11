#!/usr/bin/env node
// Builds the Blocks2Cpp specification website: one HTML page generated from
// the Markdown sources in docs/, plus its static assets, written to site/dist/.
//
// The Markdown files stay the single source of truth. The build fails (and so
// does CI) on broken links or anchors, Markdown files that would be left off the
// page, duplicate anchors, raw HTML or images in the docs, and non-HTTPS external
// links, so the published page can never silently drift from the repository.
//
// With --check-repo-markdown it builds nothing and checks every Markdown file of
// the repository instead (see checkRepositoryMarkdown): relative links must point
// to files that exist, and heading anchors to headings that exist, as GitHub
// resolves them. CI runs it in the docs job (docs/spec/09-quality-and-delivery.md §9.3).
//
// Usage: node build.mjs            (from site/, or `pnpm site:build` from the root)
//        node site/build.mjs --check-repo-markdown
// Env:   B2C_SITE_COMMIT=<sha>     commit shown in the footer (defaults to GITHUB_SHA,
//                                  then `git rev-parse HEAD`, then "a local working copy")

import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync, realpathSync, statSync } from 'node:fs';
import { copyFile, mkdir, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { Marked, Renderer } from 'marked';

const SITE_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_DIR = path.resolve(SITE_DIR, '..');
const SRC_DIR = path.join(SITE_DIR, 'src');
const OUT_DIR = path.join(SITE_DIR, 'dist');
const REPO_URL = 'https://github.com/JoelLogan/Blocks_To_CPP';

/** Folders whose Markdown files all appear on the page (repository-relative). */
const SPEC_DIR = 'docs/spec';
const ADR_DIR = 'docs/adr';
const CHAPTER_FILE = /^\d{2}-[a-z0-9-]+\.md$/;
const ADR_FILE = /^\d{4}-[a-z0-9-]+\.md$/;

/** Display names for fenced code block languages. */
const CODE_LANGUAGES = new Map([
  ['cpp', 'C++'],
  ['rust', 'Rust'],
  ['json', 'JSON'],
  ['toml', 'TOML'],
  ['ebnf', 'EBNF'],
  ['yaml', 'YAML'],
  ['markdown', 'Markdown'],
  ['sh', 'Shell'],
]);

/**
 * Paths the all-Markdown check skips (repository-relative): installed dependencies,
 * local tool state such as agent worktrees, and fuzz corpora and crash artifacts,
 * which are generated inputs rather than documents.
 */
const UNCHECKED_MARKDOWN = [/(^|\/)node_modules\//, /^\.claude\//, /^fuzz\/(corpus|artifacts)\//];

/**
 * CHANGELOG.md is checked down to its first released version only: released
 * entries are history, written against the repository as it was then, and are
 * not edited when a heading they link to is renamed later.
 */
const CHANGELOG_FILE = 'CHANGELOG.md';
const RELEASED_VERSION_HEADING = /^## (?!\[Unreleased\])/m;

/**
 * Source files whose messages name Markdown files by their repository path, such
 * as "see BUILDING.md#troubleshooting". The all-Markdown check reads each `*.md`
 * path (with its anchor, if any) in them as a link from the repository root, so
 * a reference must be written in one piece, not split across strings.
 */
const SOURCE_FILES_WITH_DOC_REFERENCES = ['apps/desktop/vite.config.ts', 'packages/b2c-core-wasm/scripts/build.mjs'];
const DOC_REFERENCE = /(?<![\w./:#-])(?:[\w.-]+\/)*[\w.-]+\.md(?:#[\w-]+)?(?![\w/])/g;

/** Static assets copied verbatim into dist/assets (paths relative to src/). */
const ASSETS = [
  'site.css',
  'site.js',
  'theme-init.js',
  'favicon.svg',
  'fonts/atkinson-hyperlegible-next-latin-wght-normal.woff2',
  'fonts/atkinson-hyperlegible-next-latin-wght-italic.woff2',
  'fonts/OFL.txt',
];

class BuildError extends Error {}

/** Errors are collected and reported together so one build shows every problem. */
const problems = [];
function problem(doc, message) {
  problems.push(`${doc ? `${doc.file}: ` : ''}${message}`);
}

/** Escapes text for use in HTML element content and double-quoted attributes. */
function escapeHtml(text) {
  return String(text)
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#39;');
}

/**
 * GitHub-compatible heading slug (github-slugger): lower-case, drop everything
 * except letters, marks, decimal and letter numbers, connector punctuation,
 * hyphens and spaces, then spaces → hyphens. Matching GitHub means
 * `file.md#anchor` links work both on github.com and here.
 * Example: "6.2 Stage ① Load and validate" → "62-stage--load-and-validate".
 */
function githubSlug(text) {
  return text
    .toLowerCase()
    .replace(/[^\p{L}\p{M}\p{Nd}\p{Nl}\p{Pc}\- ]/gu, '')
    .replaceAll(' ', '-');
}

/** Named entities decoded without a full HTML entity table (names are case-sensitive). */
const BASIC_ENTITIES = new Map([
  ['amp', '&'],
  ['lt', '<'],
  ['gt', '>'],
  ['quot', '"'],
  ['apos', "'"],
  ['AMP', '&'],
  ['LT', '<'],
  ['GT', '>'],
  ['QUOT', '"'],
]);

/**
 * Decodes character references the way CommonMark does: numeric references of
 * 1–7 decimal or 1–6 hex digits, plus the basic named entities. Any other named
 * entity would need the full HTML table, so it is passed to `onUnknown` (and kept).
 */
function decodeEntities(text, onUnknown) {
  return text.replace(/&(#[xX][0-9a-fA-F]{1,6}|#[0-9]{1,7}|[A-Za-z][A-Za-z0-9]{0,31});/g, (match, name) => {
    if (name[0] === '#') {
      const code = name[1] === 'x' || name[1] === 'X' ? parseInt(name.slice(2), 16) : parseInt(name.slice(1), 10);
      return code > 0 && code <= 0x10ffff && !(code >= 0xd800 && code <= 0xdfff) ? String.fromCodePoint(code) : '\uFFFD';
    }
    const decoded = BASIC_ENTITIES.get(name);
    if (decoded === undefined) onUnknown(match);
    return decoded ?? match;
  });
}

/**
 * Plain text of inline tokens, as GitHub sees it for slugs: markup removed and
 * character references decoded once. Text tokens are decoded from their raw
 * source, because marked has already decoded some references in `text`.
 */
function plainText(tokens, onUnknownEntity = () => {}) {
  return tokens
    .map((token) => {
      if (Array.isArray(token.tokens)) return plainText(token.tokens, onUnknownEntity);
      if (token.type === 'text') return decodeEntities(token.raw ?? token.text ?? '', onUnknownEntity);
      return token.text ?? '';
    })
    .join('');
}

/** A Markdown lexer for pre-passes over documents (no rendering). */
const lexer = new Marked({ gfm: true });

/** Yields every heading token in a token tree (headings can sit inside lists or blockquotes). */
function* headingTokens(tokens) {
  for (const token of tokens) {
    if (token.type === 'heading') yield token;
    if (Array.isArray(token.items)) for (const item of token.items) yield* headingTokens(item.tokens ?? []);
    else if (Array.isArray(token.tokens) && token.type !== 'heading') yield* headingTokens(token.tokens);
  }
}

/**
 * Splits a relative link into its percent-decoded path (`target`), its query and
 * its decoded fragment. Reports a malformed %-escape or a backslash through
 * `report` and returns null.
 */
function parseLinkTarget(href, report) {
  const hashIndex = href.indexOf('#');
  const beforeHash = hashIndex === -1 ? href : href.slice(0, hashIndex);
  const queryIndex = beforeHash.indexOf('?');
  let target;
  let fragment;
  try {
    target = decodeURIComponent(queryIndex === -1 ? beforeHash : beforeHash.slice(0, queryIndex));
    fragment = hashIndex === -1 ? '' : decodeURIComponent(href.slice(hashIndex + 1));
  } catch {
    report(`link "${href}" contains a malformed %-escape`);
    return null;
  }
  if (target.includes('\\')) {
    report(`link "${href}" uses a backslash; use "/" between path segments`);
    return null;
  }
  return { target, query: queryIndex === -1 ? '' : beforeHash.slice(queryIndex + 1), fragment };
}

/**
 * The repository path a relative link target points to: from the repository root
 * when it starts with "/" (as on GitHub), otherwise from the folder of `fromFile`.
 * Reports a target outside the repository and returns null.
 */
function repositoryPathOf(fromFile, target, href, report) {
  const repoPath = target.startsWith('/')
    ? path.posix.normalize(target.slice(1))
    : path.posix.normalize(path.posix.join(path.posix.dirname(fromFile), target));
  if (repoPath === '..' || repoPath.startsWith('../')) {
    report(`link "${href}" points outside the repository`);
    return null;
  }
  return repoPath;
}

/**
 * Lists the documents that make up the page, in reading order. Every other
 * Markdown file or subfolder in docs/spec and docs/adr is reported, so nothing
 * can be silently left off the page.
 */
async function discoverDocuments() {
  async function list(dir, pattern, description) {
    const matches = [];
    for (const entry of await readdir(path.join(REPO_DIR, ...dir.split('/')), { withFileTypes: true })) {
      if (entry.isDirectory()) {
        problem(null, `${dir}/${entry.name}/: subfolders are not published; move its documents into ${dir}/`);
      } else if (entry.name.endsWith('.md') && entry.name !== 'README.md') {
        if (pattern.test(entry.name)) matches.push(entry.name);
        else problem(null, `${dir}/${entry.name}: file name must look like ${description} to appear on the site`);
      }
    }
    return matches.sort();
  }

  const chapters = await list(SPEC_DIR, CHAPTER_FILE, '"NN-lowercase-name.md"');
  const adrs = await list(ADR_DIR, ADR_FILE, '"NNNN-lowercase-name.md"');
  if (chapters.length === 0) problem(null, `${SPEC_DIR}/ contains no chapters ("NN-lowercase-name.md" files)`);

  return [
    { file: `${SPEC_DIR}/README.md`, key: 'spec', kind: 'intro', levelOffset: 1, title: 'About this specification' },
    ...chapters.map((name) => ({ file: `${SPEC_DIR}/${name}`, key: `ch${name.slice(0, 2)}`, kind: 'chapter', levelOffset: 1 })),
    { file: `${ADR_DIR}/README.md`, key: 'adrs', kind: 'adr-index', levelOffset: 1 },
    ...adrs.map((name) => ({ file: `${ADR_DIR}/${name}`, key: `adr${name.slice(0, 4)}`, kind: 'adr', levelOffset: 2 })),
  ];
}

/**
 * GitHub anchors of a Markdown file that is not on the page. Repeated headings
 * get suffixes the way github-slugger assigns them ("a", "a-1", "a-2", and a
 * heading literally titled "A 1" after two "A"s becomes "a-1-1"). If a heading
 * uses a named entity this build cannot decode, its anchor cannot be computed
 * reliably, so the file is marked `uncertain` and anchors into it are not checked.
 * Cached per file in `cache` (the site build's cache by default).
 */
const externalAnchorCache = new Map();
function githubAnchorsOf(absolutePath, cache = externalAnchorCache) {
  let result = cache.get(absolutePath);
  if (!result) {
    const anchors = new Set();
    const occurrences = new Map();
    let uncertain = false;
    for (const heading of headingTokens(lexer.lexer(readFileSync(absolutePath, 'utf8')))) {
      const original = githubSlug(plainText(heading.tokens, () => (uncertain = true)).trim());
      let slug = original;
      while (occurrences.has(slug)) {
        occurrences.set(original, occurrences.get(original) + 1);
        slug = `${original}-${occurrences.get(original)}`;
      }
      occurrences.set(slug, 0);
      anchors.add(slug);
    }
    result = { anchors, uncertain };
    cache.set(absolutePath, result);
  }
  return result;
}

/**
 * Checks a link to a file or folder of the repository at `repoDir`: it must
 * exist, and an anchor into a Markdown file must be one of its GitHub heading
 * anchors, or with `?plain=1` (the source view) a line anchor such as `#L10`.
 * Fragments on other files (e.g. "LICENSE#L5" line anchors) are left to GitHub.
 * Returns whether the target is a folder, or null when it does not exist.
 */
function checkRepositoryTarget(repoDir, repoPath, { query, fragment }, href, report, anchorCache) {
  const absolute = path.join(repoDir, ...repoPath.split('/'));
  const relative = path.relative(repoDir, absolute);
  if (relative.startsWith('..') || path.isAbsolute(relative)) {
    report(`link "${href}" points outside the repository`);
    return null;
  }
  if (!existsSync(absolute)) {
    report(`link "${href}" points to "${repoPath}", which does not exist`);
    return null;
  }
  const isDirectory = statSync(absolute).isDirectory();
  if (fragment && !isDirectory && repoPath.endsWith('.md')) {
    // Rendered Markdown has heading anchors; the source view (?plain=1) has line anchors.
    if (/(^|&)plain=1(&|$)/.test(query)) {
      if (!/^L\d+(-L\d+)?$/.test(fragment)) {
        report(`link "${href}" uses ?plain=1, which only supports line anchors such as #L10`);
      }
    } else {
      const { anchors, uncertain } = githubAnchorsOf(absolute, anchorCache);
      if (!uncertain && !anchors.has(fragment)) {
        report(`link "${href}" points to a heading that does not exist in "${repoPath}"`);
      }
    }
  }
  return isDirectory;
}

/**
 * Splits a heading into its number and title, e.g. "3.4 Expression slots",
 * "3. The Block Language" or "ADR-0004: Project format: …".
 */
function splitHeadingNumber(text) {
  const patterns = [
    { regex: /^ADR-(\d{4}):\s+/, number: (m) => `ADR-${m[1]}` },
    { regex: /^(\d+(?:\.\d+)+)\s+/, number: (m) => m[1] },
    { regex: /^(\d+)\.\s+/, number: (m) => m[1] },
  ];
  for (const { regex, number } of patterns) {
    const match = regex.exec(text);
    if (match) return { number: number(match), title: text.slice(match[0].length), prefix: match[0] };
  }
  return { number: null, title: text, prefix: '' };
}

/**
 * Removes a heading's leading number (e.g. "3.4 ") from its inline tokens so the
 * number can be rendered separately. Returns null if the number is not plain text.
 */
function stripNumberPrefix(tokens, prefix) {
  const [first, ...rest] = tokens;
  if (!first || first.type !== 'text' || first.tokens || !first.text.startsWith(prefix)) return null;
  return [{ ...first, text: first.text.slice(prefix.length), raw: first.raw.slice(prefix.length) }, ...rest];
}

/** Resolves a Markdown link target to a page anchor or an absolute URL. */
function resolveHref(href, doc, docsByFile, pendingAnchors) {
  const broken = { url: '#', external: false };
  const report = (message) => problem(doc, message);
  if (/^https:\/\//i.test(href)) return { url: href, external: true };
  if (/^[a-z][a-z0-9+.-]*:/i.test(href) || href.startsWith('//')) {
    report(`link "${href}" must use https:// or point to a file in the repository`);
    return broken;
  }

  const parts = parseLinkTarget(href, report);
  if (parts === null) return broken;
  // A query (e.g. "?plain=1") only changes how GitHub displays a file. It is kept on
  // links to GitHub and ignored for documents on the page.
  const { target, query, fragment } = parts;
  if (query && !/^[A-Za-z0-9_.-]+=[A-Za-z0-9_.-]*(&[A-Za-z0-9_.-]+=[A-Za-z0-9_.-]*)*$/.test(query)) {
    report(`link "${href}" has a query string the site cannot pass through`);
    return broken;
  }

  // An anchor equal to a document's GitHub title slug means "the top of that document".
  const anchorIn = (targetDoc) => {
    const id = fragment === '' || fragment === targetDoc.titleSlug ? targetDoc.key : `${targetDoc.key}-${fragment}`;
    pendingAnchors.push({ doc, id, href });
    return { url: `#${id}`, external: false };
  };

  if (target === '') return anchorIn(doc);

  const repoPath = repositoryPathOf(doc.file, target, href, report);
  if (repoPath === null) return broken;

  const linkedDoc = docsByFile.get(repoPath);
  if (linkedDoc) return anchorIn(linkedDoc);
  if ((repoPath.startsWith(`${SPEC_DIR}/`) || repoPath.startsWith(`${ADR_DIR}/`)) && repoPath.endsWith('.md')) {
    report(`link "${href}" points to "${repoPath}", which is not a document on the page`);
    return broken;
  }

  const isDirectory = checkRepositoryTarget(REPO_DIR, repoPath, parts, href, report, externalAnchorCache);
  if (isDirectory === null) return broken;
  const view = isDirectory ? 'tree' : 'blob';
  const encodedPath = repoPath.split('/').map(encodeURIComponent).join('/');
  const encodedQuery = query ? `?${query}` : '';
  const encodedFragment = fragment ? `#${encodeURIComponent(fragment)}` : '';
  return { url: `${REPO_URL}/${view}/HEAD/${encodedPath}${encodedQuery}${encodedFragment}`, external: true };
}

/** Renders one document to HTML and records its headings for the table of contents. */
function renderDocument(doc, source, context) {
  const { docsByFile, usedIds, pendingAnchors } = context;
  const headings = [];
  let sawTitle = false;
  let currentHeadingId = doc.key;
  const base = new Renderer();
  const reportEntity = (entity) =>
    problem(
      doc,
      `heading uses "${entity}", which the site cannot decode; write the character itself ` +
        '(or escape the ampersand as "\\&" if it is not an entity)',
    );

  const marked = new Marked({
    gfm: true,
    breaks: false,
    renderer: {
      heading({ tokens, depth }) {
        const text = plainText(tokens, reportEntity).trim();
        let id;
        let label;
        if (depth === 1) {
          if (sawTitle) problem(doc, `has more than one level-1 heading ("${text}")`);
          sawTitle = true;
          id = doc.key;
          label = doc.title ?? text;
        } else {
          id = `${doc.key}-${githubSlug(text)}`;
          label = text;
        }
        if (usedIds.has(id)) problem(doc, `duplicate heading anchor "#${id}"`);
        usedIds.add(id);
        currentHeadingId = id;

        const level = Math.min(depth + doc.levelOffset, 6);
        let { number, title, prefix } = splitHeadingNumber(label);
        let bodyHtml;
        if (doc.title && depth === 1) {
          bodyHtml = escapeHtml(title);
        } else {
          const stripped = number ? stripNumberPrefix(tokens, prefix) : null;
          if (number && !stripped) ({ number, title } = { number: null, title: label });
          bodyHtml = this.parser.parseInline(stripped ?? tokens);
        }
        headings.push({ depth, id, number, title });

        const numberHtml = number ? `<span class="heading-num">${escapeHtml(number)}</span> ` : '';
        // Mouse convenience only: hidden from assistive tech so it does not pollute the
        // heading's accessible name; keyboard users navigate with the table of contents.
        const anchor = `<a class="heading-anchor" href="#${id}" aria-hidden="true" tabindex="-1">#</a>`;
        return `<h${level} id="${id}">${numberHtml}<span class="heading-text">${bodyHtml}</span>${anchor}</h${level}>\n`;
      },

      link({ href, title, tokens }) {
        const resolved = resolveHref(href, doc, docsByFile, pendingAnchors);
        const titleAttr = title ? ` title="${escapeHtml(title)}"` : '';
        const classAttr = resolved.external ? ' class="external"' : '';
        return `<a href="${escapeHtml(resolved.url)}"${classAttr}${titleAttr}>${this.parser.parseInline(tokens)}</a>`;
      },

      code({ text, lang }) {
        const language = (lang ?? '').trim().split(/\s+/)[0].toLowerCase();
        const display = CODE_LANGUAGES.get(language) ?? '';
        const langClass = language ? ` class="language-${escapeHtml(language)}"` : '';
        const label = display ? `<span class="code-lang">${escapeHtml(display)}</span>` : '';
        const name = display ? `${display} code` : 'Text block';
        // tabindex makes the block keyboard-scrollable; site.js removes it when nothing overflows.
        return (
          `<div class="code-block"><div class="code-bar">${label}</div>` +
          `<pre tabindex="0" role="group" aria-label="${escapeHtml(name)}"><code${langClass}>${escapeHtml(text)}</code></pre></div>\n`
        );
      },

      table(token) {
        return (
          `<div class="table-wrap" tabindex="0" role="group" aria-labelledby="${currentHeadingId}">` +
          `${base.table.call(this, token)}</div>\n`
        );
      },

      // Task lists are documentation, not forms: show the state without a form control.
      checkbox({ checked }) {
        return (
          `<span class="task-box" aria-hidden="true">${checked ? '☑' : '☐'}</span>` +
          `<span class="visually-hidden">${checked ? 'Done: ' : 'To do: '}</span>`
        );
      },

      html({ text }) {
        problem(doc, `contains raw HTML, which the site does not render: ${JSON.stringify(text.trim().slice(0, 60))}`);
        return escapeHtml(text);
      },

      image({ href }) {
        problem(doc, `contains an image ("${href}"); images are not supported on the site yet`);
        return '';
      },
    },
  });

  const html = marked.parse(source, { async: false });
  if (!sawTitle) problem(doc, 'has no level-1 heading');
  return { html, headings };
}

/** Builds the nested table-of-contents list shared by the sidebar and the mobile menu. */
function renderToc(rendered) {
  const items = [];
  let adrGroup = null;
  for (const { doc, headings } of rendered) {
    const title = headings.find((h) => h.depth === 1);
    if (!title) continue;
    if (doc.kind === 'adr') {
      adrGroup?.children.push({ id: title.id, number: title.number?.replace('ADR-', ''), title: title.title });
      continue;
    }
    const entry = {
      id: title.id,
      number: doc.kind === 'chapter' ? title.number?.padStart(2, '0') : null,
      title: title.title,
      children:
        doc.kind === 'chapter'
          ? headings.filter((h) => h.depth === 2).map((h) => ({ id: h.id, number: h.number, title: h.title }))
          : [],
    };
    items.push(entry);
    if (doc.kind === 'adr-index') adrGroup = entry;
  }

  const link = ({ id, number, title }) =>
    `<a href="#${id}">${number ? `<span class="toc-num">${escapeHtml(number)}</span>` : ''}` +
    `<span class="toc-title">${escapeHtml(title)}</span></a>`;
  const list = items
    .map((item) => {
      const children = item.children.length
        ? `<ol>${item.children.map((child) => `<li>${link(child)}</li>`).join('')}</ol>`
        : '';
      return `<li class="toc-chapter">${link(item)}${children}</li>`;
    })
    .join('\n');
  return `<ol class="toc-list">\n${list}\n</ol>`;
}

/** Returns the commit to show in the footer, validated as a hex SHA. */
function resolveCommit() {
  let commit = process.env.B2C_SITE_COMMIT || process.env.GITHUB_SHA || '';
  if (!commit) {
    try {
      commit = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: REPO_DIR, encoding: 'utf8' }).trim();
    } catch {
      commit = '';
    }
  }
  return /^[0-9a-f]{7,40}$/.test(commit) ? commit : null;
}

function shortHash(content) {
  return createHash('sha256').update(content).digest('hex').slice(0, 10);
}

/**
 * Fills {{NAME}} placeholders in one pass over the template, so text inserted
 * from the docs is never scanned for placeholders. A function replacer avoids
 * `$` replacement patterns. Unknown or unused placeholders are build errors.
 */
function fillTemplate(template, values) {
  const used = new Set();
  const output = template.replace(/\{\{([A-Z_]+)\}\}/g, (match, name) => {
    if (!Object.hasOwn(values, name)) throw new BuildError(`Template placeholder ${match} has no value`);
    used.add(name);
    return values[name];
  });
  for (const name of Object.keys(values)) {
    if (!used.has(name)) throw new BuildError(`Template is missing placeholder {{${name}}}`);
  }
  return output;
}

/**
 * The Markdown files the all-Markdown check covers, sorted: every `*.md` file git
 * tracks, plus new files it does not ignore (so a document is checked before it
 * is committed), minus UNCHECKED_MARKDOWN and files deleted from the working tree.
 */
function repositoryMarkdownFiles(repoDir) {
  const listed = execFileSync('git', ['ls-files', '-z', '--cached', '--others', '--exclude-standard', '--', '*.md'], {
    cwd: repoDir,
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024,
  });
  return [...new Set(listed.split('\0'))]
    .filter((file) => file !== '' && !UNCHECKED_MARKDOWN.some((pattern) => pattern.test(file)))
    .filter((file) => existsSync(path.join(repoDir, ...file.split('/'))))
    .sort();
}

/** The 1-based line of a character offset. */
function lineAt(source, offset) {
  let line = 1;
  for (let index = source.indexOf('\n'); index !== -1 && index < offset; index = source.indexOf('\n', index + 1)) {
    line += 1;
  }
  return line;
}

/**
 * Checks one link of the all-Markdown check, as GitHub resolves it from
 * `fromFile` (repository-relative; "" for the repository root). Links with a
 * scheme (https:, mailto:, …) are external and left to lychee.
 */
function checkRepositoryLink(repoDir, fromFile, href, report, anchorCache) {
  if (/^[a-z][a-z0-9+.-]*:/i.test(href) || href.startsWith('//')) return;
  const parts = parseLinkTarget(href, report);
  if (parts === null) return;
  const repoPath = parts.target === '' ? fromFile : repositoryPathOf(fromFile, parts.target, href, report);
  if (repoPath === null) return;
  checkRepositoryTarget(repoDir, repoPath, parts, href, report, anchorCache);
}

/**
 * The all-Markdown check (`--check-repo-markdown`). Unlike the site build, which
 * checks only the page's documents, it covers every Markdown file of the
 * repository (repositoryMarkdownFiles; CHANGELOG.md up to its first released
 * version) and the Markdown references in SOURCE_FILES_WITH_DOC_REFERENCES. Each
 * relative link and image must point to a file or folder that exists inside the
 * repository, and each anchor into a Markdown file (including the file's own) to
 * a heading GitHub gives that anchor. External links are lychee's job.
 *
 * Returns the number of Markdown files checked and the problems found, each as
 * "file:line: message".
 */
export function checkRepositoryMarkdown(repoDir = REPO_DIR) {
  const found = [];
  const anchorCache = new Map();
  const files = repositoryMarkdownFiles(repoDir);

  for (const file of files) {
    let source = readFileSync(path.join(repoDir, ...file.split('/')), 'utf8').replace(/\r\n?/g, '\n');
    if (file === CHANGELOG_FILE) {
      const history = source.search(RELEASED_VERSION_HEADING);
      if (history !== -1) source = source.slice(0, history);
    }
    // Tokens come in document order, so each link's source text is searched for
    // after the previous one's to find its line.
    let cursor = 0;
    lexer.walkTokens(lexer.lexer(source), (token) => {
      if (token.type !== 'link' && token.type !== 'image') return;
      let offset = source.indexOf(token.raw, cursor);
      if (offset === -1) offset = source.indexOf(token.raw);
      if (offset !== -1) cursor = offset + 1;
      const where = offset === -1 ? file : `${file}:${lineAt(source, offset)}`;
      checkRepositoryLink(repoDir, file, token.href, (message) => found.push(`${where}: ${message}`), anchorCache);
    });
  }

  for (const file of SOURCE_FILES_WITH_DOC_REFERENCES) {
    const absolute = path.join(repoDir, ...file.split('/'));
    if (!existsSync(absolute)) {
      found.push(`${file}: does not exist; update SOURCE_FILES_WITH_DOC_REFERENCES in site/build.mjs`);
      continue;
    }
    const source = readFileSync(absolute, 'utf8');
    for (const match of source.matchAll(DOC_REFERENCE)) {
      const where = `${file}:${lineAt(source, match.index)}`;
      checkRepositoryLink(repoDir, '', match[0], (message) => found.push(`${where}: ${message}`), anchorCache);
    }
  }
  return { files: files.length, problems: found };
}

/** Runs the all-Markdown check and reports its result; throws a BuildError listing every problem. */
function runRepositoryMarkdownCheck() {
  const { files, problems: found } = checkRepositoryMarkdown();
  if (found.length > 0) {
    throw new BuildError(
      `${found.length} broken link${found.length === 1 ? '' : 's'} or anchor${found.length === 1 ? '' : 's'} ` +
        `in the repository's Markdown:\n  - ${found.join('\n  - ')}`,
    );
  }
  console.log(
    `Checked ${files} Markdown files and ${SOURCE_FILES_WITH_DOC_REFERENCES.length} source files: ` +
      'every relative link and heading anchor resolves.',
  );
}

async function buildSite() {
  const documents = await discoverDocuments();
  const docsByFile = new Map(documents.map((doc) => [doc.file, doc]));
  const sources = new Map();
  for (const doc of documents) {
    const source = await readFile(path.join(REPO_DIR, ...doc.file.split('/')), 'utf8');
    sources.set(doc, source);
    // Pre-pass: GitHub's anchor for each document's title, so links to it resolve
    // even when the linked document has not been rendered yet.
    const title = headingTokens(lexer.lexer(source)).next().value;
    doc.titleSlug = title?.depth === 1 ? githubSlug(plainText(title.tokens).trim()) : null;
  }

  const context = { docsByFile, usedIds: new Set(['main', 'top']), pendingAnchors: [] };
  const rendered = documents.map((doc) => ({ doc, ...renderDocument(doc, sources.get(doc), context) }));

  for (const { doc, id, href } of context.pendingAnchors) {
    if (!context.usedIds.has(id)) problem(doc, `link "${href}" points to a heading that does not exist (#${id})`);
  }
  if (problems.length > 0) {
    throw new BuildError(`The site cannot be built:\n  - ${problems.join('\n  - ')}`);
  }

  const sections = rendered
    .map(({ doc, html }) => `<section class="doc doc-${doc.kind}" aria-labelledby="${doc.key}">\n${html}</section>`)
    .join('\n');

  await rm(OUT_DIR, { recursive: true, force: true });
  await mkdir(path.join(OUT_DIR, 'assets', 'fonts'), { recursive: true });
  const versions = {};
  for (const asset of ASSETS) {
    const from = path.join(SRC_DIR, ...asset.split('/'));
    await copyFile(from, path.join(OUT_DIR, 'assets', ...asset.split('/')));
    versions[asset] = shortHash(await readFile(from));
  }

  const commit = resolveCommit();
  const commitHtml = commit
    ? `<a href="${REPO_URL}/commit/${commit}">${commit.slice(0, 7)}</a>`
    : 'a local working copy';
  const toc = renderToc(rendered);
  const template = await readFile(path.join(SRC_DIR, 'index.html'), 'utf8');
  const page = fillTemplate(template, {
    CSS_VERSION: versions['site.css'],
    JS_VERSION: versions['site.js'],
    THEME_JS_VERSION: versions['theme-init.js'],
    TOC_SIDEBAR: toc,
    TOC_MOBILE: toc,
    CONTENT: sections,
    COMMIT: commitHtml,
    REPO_URL,
  });

  await writeFile(path.join(OUT_DIR, 'index.html'), page, 'utf8');
  const kib = (Buffer.byteLength(page) / 1024).toFixed(0);
  console.log(`Built site/dist/index.html (${kib} KiB) from ${documents.length} documents.`);
}

async function main(args) {
  if (args.length === 0) return buildSite();
  if (args.length === 1 && args[0] === '--check-repo-markdown') return runRepositoryMarkdownCheck();
  throw new BuildError(`Unknown arguments: ${args.join(' ')}\nUsage: node build.mjs [--check-repo-markdown]`);
}

/** Whether this file is the script Node was started with, rather than imported (by its tests). */
function isMainModule() {
  try {
    return pathToFileURL(realpathSync(process.argv[1])).href === import.meta.url;
  } catch {
    return false;
  }
}

if (isMainModule()) {
  main(process.argv.slice(2)).catch((error) => {
    console.error(error instanceof BuildError ? error.message : error);
    process.exitCode = 1;
  });
}
