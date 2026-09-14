import { useEffect, useRef } from 'preact/hooks';
import { rawUrl } from '../api/docs';
import type { DocMeta } from '../api/types';
import { build, currentPath, dirOf, doc, docError, docLoading, docUrl, formatDate, formatSize, openDoc, resolveRelative } from '../store';
import { CodeView } from './CodeView';
import { Download, ExternalLink } from './Icons';

function isRelative(href: string): boolean {
  return !/^([a-z]+:|\/|#)/i.test(href);
}

/** Rewrites relative links and images inside rendered markdown. Code blocks are already highlighted. */
function MarkdownBody({ html, path, title }: { html: string; path: string; title: string }) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const dir = dirOf(path);

    // The first h1 usually repeats the document title, which the header already shows
    const first = el.firstElementChild;
    if (first?.tagName === 'H1' && first.textContent?.trim() === title) first.remove();

    el.querySelectorAll<HTMLAnchorElement>('a[href]').forEach((a) => {
      const href = a.getAttribute('href') ?? '';
      if (isRelative(href)) {
        const [target, hash] = href.split('#');
        const resolved = resolveRelative(dir, target ?? '');
        a.setAttribute('href', docUrl(resolved) + (hash ? `#${hash}` : ''));
        a.onclick = (e) => {
          e.preventDefault();
          openDoc(resolved);
        };
      } else if (/^https?:/i.test(href)) {
        a.target = '_blank';
        a.rel = 'noopener';
      }
    });

    el.querySelectorAll<HTMLImageElement>('img[src]').forEach((img) => {
      const src = img.getAttribute('src') ?? '';
      if (isRelative(src)) img.setAttribute('src', rawUrl(resolveRelative(dir, src)));
    });
  }, [html, path, title]);

  return <div class="md" ref={ref} dangerouslySetInnerHTML={{ __html: html }} />;
}

function Breadcrumbs({ path }: { path: string }) {
  const parts = path.split('/');
  return (
    <div class="doc-path">
      {parts.map((p, i) => (
        <span key={i} class={i === parts.length - 1 ? 'doc-path-last' : ''}>
          {i > 0 && <span class="doc-path-sep">/</span>}
          {p}
        </span>
      ))}
    </div>
  );
}

function DocHeader({ meta }: { meta: DocMeta }) {
  return (
    <header class="doc-header">
      <Breadcrumbs path={meta.path} />
      <h1 class="doc-title">{meta.title}</h1>
      <div class="doc-meta">
        <span class="pill">{meta.content_type}</span>
        <span class="pill">{formatSize(meta.size)}</span>
        {meta.updated_at && <span class="pill">updated {formatDate(meta.updated_at)}</span>}
        <a class="pill pill-link" href={rawUrl(meta.path)} target="_blank" rel="noopener">
          <ExternalLink size={13} /> raw
        </a>
      </div>
    </header>
  );
}

function Welcome() {
  const b = build.value;
  const unavailable = !!b && !b.hash && !!b.error;
  const empty = !!b?.hash && b.documents === 0;

  let heading = 'Pick a document in the tree';
  let text = 'Markdown, HTML, PDF, images and sources in one place. The tree is on the left, search by name is on top.';
  if (unavailable) {
    heading = 'The archive is not loaded yet';
    text = `${b!.error}. Once the archive shows up in storage, press "Refresh" in the header.`;
  } else if (empty) {
    heading = 'The archive is empty';
    text = 'The loaded archive contained no files at all. Check that CI puts the repository content into it.';
  }

  return (
    <div class="doc-welcome">
      <p class="doc-welcome-kicker">Documentation</p>
      <h1>{heading}</h1>
      <p class="muted">{text}</p>
      <div class="welcome-cards">
        <a class="welcome-card" href="/llms.txt" target="_blank" rel="noopener">
          <span class="welcome-card-title">/llms.txt</span>
          <span class="muted">an index of every document for AI agents</span>
        </a>
        <a class="welcome-card" href="/api/tree" target="_blank" rel="noopener">
          <span class="welcome-card-title">/api/tree</span>
          <span class="muted">the directory tree as JSON</span>
        </a>
        <a class="welcome-card" href="/api/build" target="_blank" rel="noopener">
          <span class="welcome-card-title">/api/build</span>
          <span class="muted">archive state and refresh time</span>
        </a>
      </div>
    </div>
  );
}

export function DocViewer() {
  const path = currentPath.value;
  const current = doc.value;

  // After a search hit the marked match is scrolled into view once the document is on screen.
  useEffect(() => {
    if (current?.jump) document.querySelector('.search-target')?.scrollIntoView({ block: 'center' });
  }, [current]);

  if (!path) return <Welcome />;
  if (docLoading.value && !doc.value) return <div class="doc-skeleton" />;
  if (docError.value) {
    return (
      <div class="doc-status">
        <p class="message message-error">{docError.value}</p>
        <p class="muted">{path}</p>
      </div>
    );
  }
  const d = doc.value;
  if (!d) return null;

  const raw = rawUrl(d.meta.path);
  let body;
  switch (d.render.kind) {
    case 'html':
      body = <MarkdownBody html={d.render.body} path={d.meta.path} title={d.meta.title} />;
      break;
    case 'text':
      body = <CodeView code={d.render.body} html={d.highlighted} />;
      break;
    case 'frame':
      body = <iframe class="frame" src={raw} sandbox="allow-scripts allow-forms allow-popups allow-modals" title={d.meta.title} />;
      break;
    case 'pdf':
      // Without sandbox: in a sandbox Chrome refuses to start its built-in PDF viewer
      body = <iframe class="frame" src={raw} title={d.meta.title} />;
      break;
    case 'image':
      body = <img class="doc-image" src={raw} alt={d.meta.title} />;
      break;
    default:
      body = (
        <p class="doc-status">
          This format is not displayed in the browser.{' '}
          <a class="btn btn-secondary btn-sm" href={raw} download><Download size={14} /> Download</a>
        </p>
      );
  }

  return (
    <article class={`doc ${docLoading.value ? 'doc--stale' : ''}`} key={d.meta.path}>
      <DocHeader meta={d.meta} />
      {body}
    </article>
  );
}
