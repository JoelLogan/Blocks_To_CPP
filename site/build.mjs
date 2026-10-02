#!/usr/bin/env node
// Builds the Blocks2Cpp specification website: one HTML page generated from
// the Markdown sources in docs/, plus its static assets, written to site/dist/.
//
// The Markdown files stay the single source of truth. The build fails (and so
// does CI) on broken links or anchors, Markdown files that would be left off the
// page, duplicate anchors, raw HTML or images in the docs, and non-HTTPS external
// links, so the published page can never silently drift from the repository.
//
// Usage: node build.mjs            (from site/, or `pnpm site:build` from the root)
// Env:   B2C_SITE_COMMIT=<sha>     commit shown in the footer (defaults to GITHUB_SHA,
//                                  then `git rev-parse HEAD`, then "a local working copy")

import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync, statSync } from 'node:fs';
import { copyFile, mkdir, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
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

const XML_ENTITIES = new Map([
  ['amp', '&'],
  ['lt', '<'],
  ['gt', '>'],
  ['quot', '"'],
  ['apos', "'"],
]);

/**
 * Decodes the XML entities and numeric character references that marked leaves
 * in text tokens. Other named entities would need a full HTML entity table, so
 * they are reported instead (write the literal character in the Markdown).
 */
function decodeEntities(text, onUnknown) {
  return text.replace(/&(#x[0-9a-f]+|#\d+|[a-z][a-z0-9]*);/gi, (match, name) => {
    if (name[0] === '#') {
      const code = name[1] === 'x' || name[1] === 'X' ? parseInt(name.slice(2), 16) : parseInt(name.slice(1), 10);
      return code > 0 && code <= 0x10ffff ? String.fromCodePoint(code) : match;
    }
    const decoded = XML_ENTITIES.get(name.toLowerCase());
    if (decoded === undefined) onUnknown(match);
    return decoded ?? match;
  });
}

/** Plain text of inline tokens, as GitHub sees it for slugs (markup removed, entities decoded). */
function plainText(tokens, onUnknownEntity = () => {}) {
  return tokens
    .map((token) => {
      if (Array.isArray(token.tokens)) return plainText(token.tokens, onUnknownEntity);
      if (token.type === 'text') return decodeEntities(token.text ?? '', onUnknownEntity);
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

/** Decodes a percent-encoded URL part; reports and returns null if it is malformed. */
function safeDecode(doc, href, value) {
  try {
    return decodeURIComponent(value);
  } catch {
    problem(doc, `link "${href}" contains a malformed %-escape`);
    return null;
  }
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
  if (chapters.length === 0) throw new BuildError(`No specification chapters found in ${SPEC_DIR}/.`);

  return [
    { file: `${SPEC_DIR}/README.md`, key: 'spec', kind: 'intro', levelOffset: 1, title: 'About this specification' },
    ...chapters.map((name) => ({ file: `${SPEC_DIR}/${name}`, key: `ch${name.slice(0, 2)}`, kind: 'chapter', levelOffset: 1 })),
    { file: `${ADR_DIR}/README.md`, key: 'adrs', kind: 'adr-index', levelOffset: 1 },
    ...adrs.map((name) => ({ file: `${ADR_DIR}/${name}`, key: `adr${name.slice(0, 4)}`, kind: 'adr', levelOffset: 2 })),
  ];
}

/**
 * GitHub anchors of a Markdown file that is not on the page, including GitHub's
 * "-1", "-2" suffixes for repeated headings. Cached per file.
 */
const externalAnchorCache = new Map();
function githubAnchorsOf(absolutePath) {
  let anchors = externalAnchorCache.get(absolutePath);
  if (!anchors) {
    anchors = new Set();
    const seen = new Map();
    for (const heading of headingTokens(lexer.lexer(readFileSync(absolutePath, 'utf8')))) {
      const slug = githubSlug(plainText(heading.tokens).trim());
      const count = seen.get(slug) ?? 0;
      seen.set(slug, count + 1);
      anchors.add(count === 0 ? slug : `${slug}-${count}`);
    }
    externalAnchorCache.set(absolutePath, anchors);
  }
  return anchors;
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
  if (/^https:\/\//i.test(href)) return { url: href, external: true };
  if (/^[a-z][a-z0-9+.-]*:/i.test(href) || href.startsWith('//')) {
    problem(doc, `link "${href}" must use https:// or point to a file in the repository`);
    return broken;
  }

  const hashIndex = href.indexOf('#');
  const beforeHash = hashIndex === -1 ? href : href.slice(0, hashIndex);
  const fragment = hashIndex === -1 ? '' : safeDecode(doc, href, href.slice(hashIndex + 1));
  // A query (e.g. "?plain=1") only changes how GitHub displays a file; the target is the same.
  const target = safeDecode(doc, href, beforeHash.split('?')[0]);
  if (fragment === null || target === null) return broken;

  // An anchor equal to a document's GitHub title slug means "the top of that document".
  const anchorIn = (targetDoc) => {
    const id = fragment === '' || fragment === targetDoc.titleSlug ? targetDoc.key : `${targetDoc.key}-${fragment}`;
    pendingAnchors.push({ doc, id, href });
    return { url: `#${id}`, external: false };
  };

  if (target === '') return anchorIn(doc);

  const repoPath = target.startsWith('/')
    ? path.posix.normalize(target.slice(1))
    : path.posix.normalize(path.posix.join(path.posix.dirname(doc.file), target));
  if (repoPath === '..' || repoPath.startsWith('../')) {
    problem(doc, `link "${href}" points outside the repository`);
    return broken;
  }

  const linkedDoc = docsByFile.get(repoPath);
  if (linkedDoc) return anchorIn(linkedDoc);
  if ((repoPath.startsWith(`${SPEC_DIR}/`) || repoPath.startsWith(`${ADR_DIR}/`)) && repoPath.endsWith('.md')) {
    problem(doc, `link "${href}" points to "${repoPath}", which is not a document on the page`);
    return broken;
  }

  const absolute = path.join(REPO_DIR, ...repoPath.split('/'));
  if (!existsSync(absolute)) {
    problem(doc, `link "${href}" points to "${repoPath}", which does not exist`);
    return broken;
  }
  const isDirectory = statSync(absolute).isDirectory();
  // Heading anchors into other Markdown files are checked against GitHub's slugs.
  // Fragments on other files (e.g. "LICENSE#L5" line anchors) are left to GitHub.
  if (fragment && !isDirectory && repoPath.endsWith('.md') && !githubAnchorsOf(absolute).has(fragment)) {
    problem(doc, `link "${href}" points to a heading that does not exist in "${repoPath}"`);
  }
  const view = isDirectory ? 'tree' : 'blob';
  const encodedPath = repoPath.split('/').map(encodeURIComponent).join('/');
  const encodedFragment = fragment ? `#${encodeURIComponent(fragment)}` : '';
  return { url: `${REPO_URL}/${view}/HEAD/${encodedPath}${encodedFragment}`, external: true };
}

/** Renders one document to HTML and records its headings for the table of contents. */
function renderDocument(doc, source, context) {
  const { docsByFile, usedIds, pendingAnchors } = context;
  const headings = [];
  let sawTitle = false;
  let currentHeadingId = doc.key;
  const base = new Renderer();
  const reportEntity = (entity) =>
    problem(doc, `heading uses the HTML entity "${entity}"; write the character itself instead`);

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

async function main() {
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

main().catch((error) => {
  console.error(error instanceof BuildError ? error.message : error);
  process.exitCode = 1;
});
