/**
 * Search matches inside a prepared document. Works on HTML strings before the document is shown,
 * so the marks appear together with the document. DOMParser runs no scripts.
 */

/** Where a search hit points: the line in the source file and the match rules of the query. */
export interface SearchTarget {
  line: number;
  query: string;
  regex: boolean;
  caseSensitive: boolean;
  /** The number of the match in the file, counted from zero, to find it in rendered markdown. */
  occurrence: number;
}

/** Rendered markdown with more matches than this is marked only up to it. */
const MAX_MARKS = 2000;

function escapeRegExp(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

function escapeHtml(s: string): string {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

/**
 * The server's match rules in the browser: smart case unless asked otherwise.
 * Null when the browser cannot read the pattern; the document then opens without marks.
 */
export function searchRegExp(t: Pick<SearchTarget, 'query' | 'regex' | 'caseSensitive'>): RegExp | null {
  const insensitive = !t.caseSensitive && t.query === t.query.toLowerCase();
  const source = t.regex ? t.query : escapeRegExp(t.query);
  for (const flags of ['gu', 'g']) {
    try {
      return new RegExp(source, insensitive ? `${flags}i` : flags);
    } catch {
      /* try without the unicode flag, then give up */
    }
  }
  return null;
}

/** Plain text as numbered lines, so a hit in a file without highlighting has a line to point at. */
export function plainLines(text: string): string {
  const lines = text.replace(/\r?\n$/, '').split(/\r?\n/);
  const body = lines.map((l) => `<span class="line">${escapeHtml(l)}</span>`).join('\n');
  return `<pre class="shiki shiki-plain"><code>${body}</code></pre>`;
}

/** Marks the line of a hit in numbered code. */
export function markLine(html: string, line: number): string {
  const doc = new DOMParser().parseFromString(html, 'text/html');
  const el = doc.querySelectorAll('.line')[line - 1];
  if (!el) return html;
  el.classList.add('line--hit', 'search-target');
  return doc.body.innerHTML;
}

/**
 * Wraps matches in rendered markdown into <mark>. The source line does not map onto rendered HTML,
 * so the target is the match with the same number as in the source file.
 */
export function markText(html: string, t: SearchTarget): string {
  const re = searchRegExp(t);
  if (!re) return html;
  const doc = new DOMParser().parseFromString(html, 'text/html');

  const walker = doc.createTreeWalker(doc.body, NodeFilter.SHOW_TEXT);
  const nodes: Text[] = [];
  for (let n = walker.nextNode(); n; n = walker.nextNode()) nodes.push(n as Text);

  const marks: HTMLElement[] = [];
  for (const node of nodes) {
    if (marks.length >= MAX_MARKS) break;
    const text = node.data;
    const frag = doc.createDocumentFragment();
    let last = 0;
    re.lastIndex = 0;
    for (let m = re.exec(text); m && marks.length < MAX_MARKS; m = re.exec(text)) {
      if (m[0] === '') {
        re.lastIndex++;
        continue;
      }
      frag.append(text.slice(last, m.index));
      const mark = doc.createElement('mark');
      mark.className = 'search-hit';
      mark.textContent = m[0];
      frag.append(mark);
      marks.push(mark);
      last = m.index + m[0].length;
    }
    if (last === 0) continue;
    frag.append(text.slice(last));
    node.replaceWith(frag);
  }

  if (!marks.length) return html;
  marks[Math.min(t.occurrence, marks.length - 1)].classList.add('search-target');
  return doc.body.innerHTML;
}
