import { computed, signal } from '@preact/signals';
import { ApiError } from './api/client';
import { getBuild, getDoc, getTree, encodePath, postRefresh, searchDocs, type SearchParams } from './api/docs';
import { highlight, highlightMarkdown, langForPath, preload } from './highlight';
import type { BuildInfo, DocResponse, SearchFile, SearchHit, SearchResponse, TreeNode } from './api/types';
import { markLine, markText, plainLines, type SearchTarget } from './marks';

// The document tree (cached on the server, here the last loaded copy)
export const tree = signal<TreeNode | null>(null);
export const treeError = signal('');

// The open document
export const currentPath = signal<string>(pathFromLocation());
export const doc = signal<DocResponse | null>(null);
export const docError = signal('');
export const docLoading = signal(false);

// Expanded directories and the sidebar filter
export const expanded = signal<Set<string>>(new Set());
export const filter = signal('');

export function pathFromLocation(): string {
  const p = location.pathname;
  if (!p.startsWith('/docs/')) return '';
  return p.slice('/docs/'.length).split('/').map(decodeURIComponent).join('/');
}

export function docUrl(path: string): string {
  return `/docs/${encodePath(path)}`;
}

// Build details: what went into this image
export const build = signal<BuildInfo | null>(null);

export async function loadBuild(): Promise<void> {
  try {
    build.value = await getBuild();
  } catch {
    /* the header will simply carry no build pill */
  }
}

// Refreshing the archive from the button
export const refreshing = signal(false);
export const refreshNotice = signal<{ kind: 'ok' | 'info' | 'error'; text: string } | null>(null);
let noticeTimer: number | undefined;

function notify(kind: 'ok' | 'info' | 'error', text: string): void {
  refreshNotice.value = { kind, text };
  clearTimeout(noticeTimer);
  noticeTimer = window.setTimeout(() => (refreshNotice.value = null), 5000);
}

/** The content changed: re-read the tree and the open document. */
async function applyNewContent(): Promise<void> {
  await loadTree();
  const path = currentPath.value || build.value?.readme;
  if (path) await openDoc(path, false);
}

export async function refreshContent(): Promise<void> {
  if (refreshing.value) return;
  refreshing.value = true;
  try {
    const res = await postRefresh();
    build.value = res.build;
    if (res.result === 'updated') {
      await applyNewContent();
      notify('ok', 'Documentation refreshed');
    } else if (res.result === 'unchanged') {
      notify('info', 'No changes');
    } else if (res.result === 'busy') {
      notify('info', 'A refresh is already running');
    } else {
      notify('error', res.message);
    }
  } catch {
    notify('error', 'Refresh failed');
  } finally {
    refreshing.value = false;
  }
}

/** Someone else may have refreshed the documentation: check again when the tab regains focus. */
export async function syncWithServer(): Promise<void> {
  const prev = build.value?.hash ?? null;
  try {
    const next = await getBuild();
    build.value = next;
    if (next.hash && next.hash !== prev) await applyNewContent();
  } catch {
    /* the server is unavailable, keep showing what we had */
  }
}

export async function loadTree(): Promise<void> {
  try {
    tree.value = await getTree();
    treeError.value = '';
  } catch (e) {
    treeError.value = e instanceof Error ? e.message : 'Failed to load the tree';
  }
}

/** Highlighting is not awaited longer than this: plain text is shown so the document is not held up. */
const HIGHLIGHT_WAIT_MS = 1500;

function withTimeout<T>(promise: Promise<T>, ms: number, fallback: T): Promise<T> {
  return Promise.race([
    promise.catch(() => fallback),
    new Promise<T>((resolve) => setTimeout(() => resolve(fallback), ms)),
  ]);
}

/**
 * Prepares the document for display in full. Highlighting and search marks are computed here rather
 * than after render, so the document appears once and plain text is never seen turning into
 * highlighted text.
 */
async function prepare(res: DocResponse, target: SearchTarget | null): Promise<DocResponse> {
  if (res.render.kind === 'text') {
    const lang = langForPath(res.meta.path);
    let highlighted = lang
      ? await withTimeout(highlight(res.render.body, lang), HIGHLIGHT_WAIT_MS, null)
      : null;
    // A hit needs line numbers to point at, even in a file without highlighting.
    if (target) highlighted = markLine(highlighted ?? plainLines(res.render.body), target.line);
    return { ...res, highlighted, jump: !!target };
  }
  if (res.render.kind === 'html') {
    let body = await withTimeout(highlightMarkdown(res.render.body), HIGHLIGHT_WAIT_MS, res.render.body);
    if (target) body = markText(body, target);
    return { ...res, render: { kind: 'html', body }, jump: !!target };
  }
  return res;
}

// The number of the latest open: a response to a stale request must not overwrite a newer document.
let openSeq = 0;

export async function openDoc(path: string, push = true, target: SearchTarget | null = null): Promise<void> {
  const seq = ++openSeq;
  if (push && path !== currentPath.value) history.pushState(null, '', docUrl(path));
  currentPath.value = path;
  expandTo(path);
  docLoading.value = true;

  // The core and the grammar load in parallel with the document request. For markdown the block
  // language is not known up front, so only the core is loaded.
  const lang = langForPath(path);
  preload(lang === 'markdown' ? null : lang);

  try {
    const prepared = await prepare(await getDoc(path), target);
    if (seq !== openSeq) return;
    doc.value = prepared;
    docError.value = '';
    document.title = `${prepared.meta.title} — adocs`;
  } catch (e) {
    if (seq !== openSeq) return;
    doc.value = null;
    docError.value = e instanceof ApiError && e.status === 404 ? 'Document not found' : 'Failed to load the document';
  } finally {
    if (seq === openSeq) docLoading.value = false;
  }
}

export function toggleDir(path: string): void {
  const next = new Set(expanded.value);
  if (next.has(path)) next.delete(path);
  else next.add(path);
  expanded.value = next;
}

/** Expands every directory on the way to a document. */
export function expandTo(path: string): void {
  const parts = path.split('/');
  const next = new Set(expanded.value);
  for (let i = 1; i < parts.length; i++) next.add(parts.slice(0, i).join('/'));
  expanded.value = next;
}

/** A document's directory: `a/b/c.md` -> `a/b`. */
export function dirOf(path: string): string {
  const i = path.lastIndexOf('/');
  return i === -1 ? '' : path.slice(0, i);
}

/** Resolves a relative link from a document into a path inside the store. */
export function resolveRelative(fromDir: string, href: string): string {
  const parts = fromDir ? fromDir.split('/') : [];
  for (const seg of href.split('/')) {
    if (seg === '' || seg === '.') continue;
    if (seg === '..') parts.pop();
    else parts.push(decodeURIComponent(seg));
  }
  return parts.join('/');
}

export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

export function formatDate(iso: string): string {
  return new Date(iso).toLocaleString(undefined, { dateStyle: 'short', timeStyle: 'short' });
}

// -- Full-text search --

export const searchQuery = signal('');
export const searchRegex = signal(false);
export const searchCase = signal(false);
export const searchOpen = signal(false);
export const searchResults = signal<SearchResponse | null>(null);
export const searchError = signal('');
export const searchLoading = signal(false);

/** Shorter queries are not sent: the server rejects them and they would match almost everything. */
export const MIN_QUERY_CHARS = 2;
/** Searching starts this long after the last keystroke, so typing does not send a request per letter. */
const SEARCH_DELAY_MS = 250;

/** The query the shown results belong to: a hit opens the document with the same match rules. */
let searched: SearchParams | null = null;
let searchAbort: AbortController | null = null;
let searchTimer: number | undefined;

function clearSearch(): void {
  searchAbort?.abort();
  searchAbort = null;
  searchLoading.value = false;
  searchResults.value = null;
  searchError.value = '';
}

/** The query was edited in the header or in the dialog: search as soon as it is long enough. */
export function setSearchQuery(value: string): void {
  searchQuery.value = value;
  clearTimeout(searchTimer);
  if (value.trim().length < MIN_QUERY_CHARS) {
    clearSearch();
    return;
  }
  searchOpen.value = true;
  searchTimer = window.setTimeout(runSearch, SEARCH_DELAY_MS);
}

/** Searches right away: Enter and the toggles do not wait for the typing delay. */
export async function runSearch(): Promise<void> {
  clearTimeout(searchTimer);
  searchOpen.value = true;
  const query = searchQuery.value;
  if (query.trim().length < MIN_QUERY_CHARS) {
    clearSearch();
    return;
  }

  // A newer search cancels the one still running.
  searchAbort?.abort();
  const abort = new AbortController();
  searchAbort = abort;
  const params = { query, regex: searchRegex.value && !!build.value?.search_regex, caseSensitive: searchCase.value };
  searchLoading.value = true;
  searchError.value = '';
  try {
    const res = await searchDocs(params, abort.signal);
    searched = params;
    searchResults.value = res;
  } catch (e) {
    if (abort.signal.aborted) return;
    searchResults.value = null;
    searchError.value = e instanceof ApiError ? e.message : 'Search failed';
  } finally {
    if (searchAbort === abort) searchLoading.value = false;
  }
}

export function closeSearch(): void {
  clearTimeout(searchTimer);
  searchOpen.value = false;
}

/** Opens the document of a hit, marks the match and scrolls to it. */
export function openSearchHit(file: SearchFile, hit: SearchHit): void {
  if (!searched) return;
  const occurrence = file.matches
    .filter((h) => h.line < hit.line)
    .reduce((n, h) => n + Math.max(1, h.ranges.length), 0);
  searchOpen.value = false;
  openDoc(file.path, true, { ...searched, line: hit.line, occurrence });
}

// -- Layout of the viewer page --

const NARROW_QUERY = '(max-width: 1024px)';

function readPref(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function savePref(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch { /* private mode */ }
}

/** The user collapsed the tree by hand. */
export const sidebarCollapsed = signal(readPref('adocs.sidebar') === 'collapsed');
/** Wide reading mode for the document. */
export const wide = signal(readPref('adocs.wide') === '1');
/** Narrow screen: the tree collapses automatically. */
export const narrow = signal(window.matchMedia(NARROW_QUERY).matches);
/** The tree is temporarily shown over the content (button hover). */
export const peek = signal(false);

export const treeHidden = computed(() => narrow.value || sidebarCollapsed.value);

window.matchMedia(NARROW_QUERY).addEventListener('change', (e) => {
  narrow.value = e.matches;
  peek.value = false;
});

export function toggleSidebar(): void {
  if (narrow.value) {
    peek.value = !peek.value;
    return;
  }
  sidebarCollapsed.value = !sidebarCollapsed.value;
  peek.value = false;
  savePref('adocs.sidebar', sidebarCollapsed.value ? 'collapsed' : 'open');
}

export function toggleWide(): void {
  wide.value = !wide.value;
  savePref('adocs.wide', wide.value ? '1' : '0');
}

let peekTimer: number | undefined;

export function openPeek(): void {
  clearTimeout(peekTimer);
  if (treeHidden.value) peek.value = true;
}

export function closePeekSoon(): void {
  clearTimeout(peekTimer);
  peekTimer = window.setTimeout(() => (peek.value = false), 300);
}

export function closePeek(): void {
  clearTimeout(peekTimer);
  peek.value = false;
}
