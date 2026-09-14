import { build, refreshContent, refreshNotice, refreshing, runSearch, searchQuery, setSearchQuery } from '../store';
import { Refresh, Search } from './Icons';

function formatTime(iso: string | null | undefined): string | null {
  return iso ? new Date(iso).toLocaleString(undefined, { dateStyle: 'short', timeStyle: 'short' }) : null;
}

function statusTitle(): string {
  const b = build.value;
  if (!b) return '';
  const lines = [`Archive: ${b.archive}`];
  if (b.hash) lines.push(`sha256: ${b.hash.slice(0, 12)}…`);
  lines.push(`Files: ${b.documents}`);
  const updated = formatTime(b.updated_at);
  if (updated) lines.push(`Updated: ${updated}`);
  const modified = formatTime(b.archive_modified_at);
  if (modified) lines.push(`Archive changed in storage: ${modified}`);
  const checked = formatTime(b.checked_at);
  if (checked) lines.push(`Last check: ${checked}`);
  if (b.error) lines.push(`Error: ${b.error}`);
  return lines.join('\n');
}

function statusLabel(): string | null {
  const b = build.value;
  if (!b) return null;
  if (!b.hash) return b.error ? 'archive unavailable' : 'not loaded';
  const updated = formatTime(b.updated_at);
  return updated ? `updated ${updated}` : null;
}

/** Full-text search. Results open in a dialog as soon as the query is long enough. */
function SearchField() {
  return (
    <form
      class="nav-search"
      role="search"
      onSubmit={(e) => {
        e.preventDefault();
        runSearch();
      }}
    >
      <button type="submit" class="nav-search-btn" title="Search" aria-label="Search">
        <Search size={15} />
      </button>
      <input
        value={searchQuery.value}
        placeholder="Search the documentation"
        aria-label="Search the documentation"
        onInput={(e) => setSearchQuery((e.target as HTMLInputElement).value)}
      />
    </form>
  );
}

interface Props {
  left?: preact.ComponentChildren;
}

export function Nav({ left }: Props) {
  const b = build.value;
  const label = statusLabel();
  const notice = refreshNotice.value;
  const busy = refreshing.value;

  return (
    <nav class="app-nav">
      {left}
      <a href="/" class="logo">adocs</a>
      <div class="app-nav-links">
        <a href="/" class="active">Documentation</a>
        <a href="/llms.txt" target="_blank" rel="noopener">For AI</a>
      </div>
      <SearchField />
      <span class="app-nav-spacer" />
      {notice && <span class={`nav-notice nav-notice--${notice.kind}`} title={notice.text}>{notice.text}</span>}
      {label && (
        <span class={`pill build-pill ${b?.error ? 'build-pill--error' : ''}`} title={statusTitle()}>
          {label}
        </span>
      )}
      <button
        type="button"
        class={`icon-btn ${busy ? 'icon-btn--on' : ''}`}
        title="Refresh the documentation from the archive"
        aria-label="Refresh the documentation"
        disabled={busy}
        onClick={refreshContent}
      >
        <Refresh size={17} class={busy ? 'spin' : ''} />
      </button>
    </nav>
  );
}
