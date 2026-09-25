import type { HighlighterCore, LanguageInput } from 'shiki/core';

// The dark theme is on screen; the light one is carried in --shiki-light variables for printing.
const THEMES = { dark: 'github-dark-default', light: 'github-light-default' };

/**
 * Grammars for the supported languages. Each one lands in its own build chunk
 * and is loaded only on the first file of that type.
 */
const GRAMMARS: Record<string, LanguageInput> = {
  markdown: () => import('@shikijs/langs/markdown'),
  go: () => import('@shikijs/langs/go'),
  rust: () => import('@shikijs/langs/rust'),
  shellscript: () => import('@shikijs/langs/shellscript'),
  yaml: () => import('@shikijs/langs/yaml'),
  toml: () => import('@shikijs/langs/toml'),
  json: () => import('@shikijs/langs/json'),
  typescript: () => import('@shikijs/langs/typescript'),
  javascript: () => import('@shikijs/langs/javascript'),
};

/** File extensions and language names in markdown blocks, mapped to a grammar. */
const ALIASES: Record<string, string> = {
  md: 'markdown', markdown: 'markdown',
  go: 'go', golang: 'go',
  rs: 'rust', rust: 'rust',
  sh: 'shellscript', bash: 'shellscript', shell: 'shellscript', zsh: 'shellscript', shellscript: 'shellscript',
  yml: 'yaml', yaml: 'yaml',
  toml: 'toml',
  json: 'json',
  ts: 'typescript', mts: 'typescript', cts: 'typescript', typescript: 'typescript',
  js: 'javascript', mjs: 'javascript', cjs: 'javascript', javascript: 'javascript',
};

/** Above this size nothing is highlighted: parsing runs on the main thread and would freeze the tab. */
const MAX_HIGHLIGHT_CHARS = 300 * 1024;

export function langForPath(path: string): string | null {
  const name = path.split('/').pop() ?? '';
  const dot = name.lastIndexOf('.');
  return dot > 0 ? ALIASES[name.slice(dot + 1).toLowerCase()] ?? null : null;
}

export function langForName(name: string): string | null {
  return ALIASES[name.toLowerCase()] ?? null;
}

let highlighter: Promise<HighlighterCore> | null = null;

/** The core and the engine are loaded lazily too: a page without code does not pay for them. */
function core(): Promise<HighlighterCore> {
  highlighter ??= Promise.all([import('shiki/core'), import('shiki/engine/javascript')]).then(
    ([{ createHighlighterCore }, { createJavaScriptRegexEngine }]) =>
      createHighlighterCore({
        themes: [import('@shikijs/themes/github-dark-default'), import('@shikijs/themes/github-light-default')],
        langs: [],
        // The JavaScript regular expression engine: no WASM and no extra weight.
        // forgiving skips the rare constructs the engine does not support instead of throwing.
        engine: createJavaScriptRegexEngine({ forgiving: true }),
      }),
  );
  return highlighter;
}

/** A grammar is loaded once, even when preloading and highlighting ask for it at the same time. */
const languages = new Map<string, Promise<void>>();

async function withLanguage(lang: string): Promise<HighlighterCore | null> {
  const grammar = GRAMMARS[lang];
  if (!grammar) return null;
  const h = await core();
  let loading = languages.get(lang);
  if (!loading) {
    loading = h.loadLanguage(grammar);
    languages.set(lang, loading);
  }
  await loading;
  return h;
}

/** Starts loading the core and the grammar up front, while the document request is in flight. */
export function preload(lang: string | null): void {
  const loading = lang ? withLanguage(lang) : core();
  loading.catch(() => {
    /* the error will surface during highlighting, where plain text is shown */
  });
}

/** Highlighted HTML, or null when the language is unsupported or the file is too large. */
export async function highlight(code: string, lang: string): Promise<string | null> {
  if (code.length > MAX_HIGHLIGHT_CHARS) return null;
  const h = await withLanguage(lang);
  if (!h) return null;
  // A trailing newline would add one more empty numbered line.
  return h.codeToHtml(code.replace(/\n$/, ''), { lang, themes: THEMES, defaultColor: 'dark' });
}

/**
 * Highlights code blocks in HTML rendered from markdown and returns new HTML.
 * pulldown-cmark marks blocks with a language-<name> class. Parsing via DOMParser runs no scripts.
 */
export async function highlightMarkdown(html: string): Promise<string> {
  if (!html.includes('class="language-')) return html;
  const parsed = new DOMParser().parseFromString(html, 'text/html');
  const blocks = Array.from(parsed.querySelectorAll<HTMLElement>('pre > code[class*="language-"]'));

  await Promise.all(
    blocks.map(async (codeEl) => {
      const cls = Array.from(codeEl.classList).find((c) => c.startsWith('language-'));
      const lang = cls ? langForName(cls.slice('language-'.length)) : null;
      const pre = codeEl.parentElement;
      if (!lang || !pre) return;
      const out = await highlight(codeEl.textContent ?? '', lang).catch(() => null);
      if (!out) return;
      const wrap = parsed.createElement('div');
      wrap.className = 'code-view';
      wrap.innerHTML = out;
      pre.replaceWith(wrap);
    }),
  );
  return parsed.body.innerHTML;
}
