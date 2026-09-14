import { useEffect, useRef } from 'preact/hooks';
import type { SearchFile } from '../api/types';
import {
  build, closeSearch, MIN_QUERY_CHARS, openSearchHit, runSearch, searchCase, searchError, searchLoading,
  searchOpen, searchQuery, searchRegex, searchResults, setSearchQuery,
} from '../store';
import { Close, File, Search } from './Icons';

/** A line with its matches marked. Ranges are character offsets, so the text is split by code points. */
function Marked({ text, ranges }: { text: string; ranges: [number, number][] }) {
  const chars = Array.from(text);
  const parts: preact.ComponentChildren[] = [];
  let pos = 0;
  ranges.forEach(([start, end], i) => {
    if (start > pos) parts.push(chars.slice(pos, start).join(''));
    parts.push(<mark key={i} class="search-hit">{chars.slice(start, end).join('')}</mark>);
    pos = end;
  });
  if (pos < chars.length) parts.push(chars.slice(pos).join(''));
  return <>{parts}</>;
}

function FileGroup({ file }: { file: SearchFile }) {
  const hidden = file.count - file.matches.length;
  return (
    <section class="search-file">
      <button type="button" class="search-file-head" onClick={() => openSearchHit(file, file.matches[0])}>
        <File size={14} />
        <span class="search-file-title">{file.title}</span>
        <span class="search-file-path">{file.path}</span>
        <span class="pill">{file.count}</span>
      </button>
      {file.matches.map((hit) => (
        <button type="button" key={hit.line} class="search-line" onClick={() => openSearchHit(file, hit)}>
          <span class="search-line-no">{hit.line}</span>
          <span class="search-line-text"><Marked text={hit.text} ranges={hit.ranges} /></span>
        </button>
      ))}
      {hidden > 0 && <p class="search-more">{hidden} more {hidden === 1 ? 'line' : 'lines'} in this file</p>}
    </section>
  );
}

function Results() {
  const res = searchResults.value;
  const loading = searchLoading.value;

  if (searchQuery.value.trim().length < MIN_QUERY_CHARS) {
    return <p class="search-empty">Type at least {MIN_QUERY_CHARS} characters to search</p>;
  }
  if (searchError.value) return <p class="message message-error search-message">{searchError.value}</p>;
  if (!res) {
    return loading ? (
      <div class="search-results">
        {[70, 90, 55, 80].map((w, i) => <div key={i} class="search-skeleton" style={{ width: `${w}%` }} />)}
      </div>
    ) : null;
  }
  if (!res.files.length) {
    return <p class="search-empty">Nothing found in {res.files_searched} files</p>;
  }
  return (
    <div class={`search-results ${loading ? 'search-results--stale' : ''}`}>
      {res.files.map((file) => <FileGroup key={file.path} file={file} />)}
    </div>
  );
}

function Status() {
  const res = searchResults.value;
  if (!res || searchError.value) return null;
  const files = res.files.length;
  return (
    <div class="search-status">
      <span>
        {res.total_matches} {res.total_matches === 1 ? 'line' : 'lines'} in {files} {files === 1 ? 'file' : 'files'}
      </span>
      <span class="search-status-dim">{res.elapsed_ms} ms</span>
      {res.truncated && <span class="search-status-warn">showing the first results, refine the query</span>}
    </div>
  );
}

/** Search results over the page. Opens as soon as the query in the header is long enough. */
export function SearchDialog() {
  const input = useRef<HTMLInputElement>(null);
  const open = searchOpen.value;

  useEffect(() => {
    if (!open) return;
    // Typing started in the header continues here, so the caret goes to the end of the query.
    const el = input.current;
    if (el) {
      el.focus();
      el.setSelectionRange(el.value.length, el.value.length);
    }
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') closeSearch();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [open]);

  if (!open) return null;
  const regexAllowed = !!build.value?.search_regex;

  function toggle(flag: typeof searchCase) {
    flag.value = !flag.value;
    runSearch();
  }

  return (
    <div class="search-backdrop" onClick={closeSearch}>
      <div
        class="search-dialog"
        role="dialog"
        aria-modal="true"
        aria-label="Search results"
        onClick={(e) => e.stopPropagation()}
      >
        <form
          class="search-bar"
          onSubmit={(e) => {
            e.preventDefault();
            runSearch();
          }}
        >
          <Search size={17} />
          <input
            ref={input}
            value={searchQuery.value}
            placeholder="Search the documentation"
            aria-label="Search the documentation"
            onInput={(e) => setSearchQuery((e.target as HTMLInputElement).value)}
          />
          <button
            type="button"
            class={`icon-btn search-toggle ${searchCase.value ? 'icon-btn--on' : ''}`}
            title="Match case"
            aria-pressed={searchCase.value}
            onClick={() => toggle(searchCase)}
          >
            Aa
          </button>
          {regexAllowed && (
            <button
              type="button"
              class={`icon-btn search-toggle ${searchRegex.value ? 'icon-btn--on' : ''}`}
              title="Regular expression"
              aria-pressed={searchRegex.value}
              onClick={() => toggle(searchRegex)}
            >
              .*
            </button>
          )}
          <button type="button" class="icon-btn" title="Close" aria-label="Close" onClick={closeSearch}>
            <Close size={17} />
          </button>
        </form>
        <Status />
        <Results />
      </div>
    </div>
  );
}
