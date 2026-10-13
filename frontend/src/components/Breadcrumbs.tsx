import { docUrl, openDoc } from '../store';

/** The path of a document or directory; every directory on the way opens as a page. */
export function Breadcrumbs({ path }: { path: string }) {
  const parts = path.split('/');
  return (
    <div class="doc-path">
      {parts.map((p, i) => {
        const last = i === parts.length - 1;
        const dir = parts.slice(0, i + 1).join('/');
        return (
          <span key={i} class={last ? 'doc-path-last' : ''}>
            {i > 0 && <span class="doc-path-sep">/</span>}
            {last ? (
              p
            ) : (
              <a
                class="doc-path-link"
                href={docUrl(dir)}
                onClick={(e) => {
                  if (e.metaKey || e.ctrlKey || e.shiftKey || e.button !== 0) return;
                  e.preventDefault();
                  openDoc(dir);
                }}
              >
                {p}
              </a>
            )}
          </span>
        );
      })}
    </div>
  );
}
