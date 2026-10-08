import { rawUrl } from './api/docs';
import { escapeHtml } from './marks';
import { dirOf, docUrl, resolveRelative } from './store';

/** A picture larger than this is left as a link: a Google document should not grow by tens of
 *  megabytes because of one screenshot. */
const MAX_INLINE_BYTES = 4 * 1024 * 1024;

export function isRelative(href: string): boolean {
  return !/^([a-z]+:|\/|#)/i.test(href);
}

/** The file as a `data:` address, or null when it is unreachable or too large. */
async function inline(url: string): Promise<string | null> {
  const res = await fetch(url);
  // A size known up front saves downloading a picture that would be dropped anyway
  if (!res.ok || Number(res.headers.get('content-length')) > MAX_INLINE_BYTES) return null;
  const blob = await res.blob();
  if (blob.size > MAX_INLINE_BYTES) return null;
  return new Promise<string | null>((resolve) => {
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result as string);
    reader.onerror = () => resolve(null);
    reader.readAsDataURL(blob);
  });
}

/** Code travels with the colours of the light theme that Shiki emits beside the dark one: the page
 *  it lands on is white, and there the dark theme is unreadable. Same rule as the "Print" button. */
function lightenCode(page: Document): void {
  page.querySelectorAll<HTMLElement>('[style*="--shiki-light"]').forEach((el) => {
    const style = el.getAttribute('style') ?? '';
    const color = /--shiki-light:\s*([^;]+)/.exec(style)?.[1]?.trim();
    const background = /--shiki-light-bg:\s*([^;]+)/.exec(style)?.[1]?.trim();
    const parts = [color && `color:${color}`, background && `background-color:${background}`].filter(Boolean);
    el.setAttribute('style', parts.join(';'));
  });
}

/** The document as a single standalone HTML file: pictures are embedded and relative links point
 *  back at the portal, so the page stays whole outside it. Google turns such a file into an
 *  ordinary Google document, keeping the headings, lists, tables and pictures. */
export async function documentHtml(path: string, title: string, body: string): Promise<string> {
  const page = new DOMParser().parseFromString(body, 'text/html');
  const dir = dirOf(path);
  lightenCode(page);

  page.querySelectorAll<HTMLAnchorElement>('a[href]').forEach((a) => {
    const href = a.getAttribute('href') ?? '';
    if (!isRelative(href)) return;
    const [target, hash] = href.split('#');
    const resolved = docUrl(resolveRelative(dir, target ?? ''));
    a.setAttribute('href', new URL(resolved + (hash ? `#${hash}` : ''), location.href).href);
  });

  // Google cannot reach inside the contour for a picture, so it travels inside the file
  await Promise.all(
    Array.from(page.querySelectorAll<HTMLImageElement>('img[src]')).map(async (img) => {
      const src = img.getAttribute('src') ?? '';
      const url = new URL(isRelative(src) ? rawUrl(resolveRelative(dir, src)) : src, location.href).href;
      img.setAttribute('src', (await inline(url).catch(() => null)) ?? url);
    }),
  );

  return `<!doctype html>
<html><head><meta charset="utf-8"><title>${escapeHtml(title)}</title></head>
<body>${page.body.innerHTML}</body></html>`;
}

/** An HTML document from the archive as a single file. Its styles and pictures lie next to it in
 *  the archive and would be lost outside the portal, so they move inside the file. Scripts are
 *  dropped: nothing runs them here, and content they would have built is missing anyway - the same
 *  limit as the PDF of such a document. */
export async function archiveHtml(path: string): Promise<string> {
  const base = new URL(rawUrl(path), location.href).href;
  const res = await fetch(base);
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  const page = new DOMParser().parseFromString(await res.text(), 'text/html');

  page.querySelectorAll('script').forEach((el) => el.remove());

  await Promise.all([
    ...Array.from(page.querySelectorAll<HTMLLinkElement>('link[rel~="stylesheet"][href]')).map(async (link) => {
      const css = await fetch(new URL(link.getAttribute('href') ?? '', base).href)
        .then((r) => (r.ok ? r.text() : null))
        .catch(() => null);
      if (css === null) return;
      const style = page.createElement('style');
      style.textContent = css;
      link.replaceWith(style);
    }),
    ...Array.from(page.querySelectorAll<HTMLImageElement>('img[src]')).map(async (img) => {
      const src = new URL(img.getAttribute('src') ?? '', base).href;
      img.setAttribute('src', (await inline(src).catch(() => null)) ?? src);
    }),
  ]);

  page.querySelectorAll<HTMLAnchorElement>('a[href]').forEach((a) => {
    const href = a.getAttribute('href') ?? '';
    if (!href.startsWith('#')) a.setAttribute('href', new URL(href, base).href);
  });

  return `<!doctype html>\n${page.documentElement.outerHTML}`;
}
