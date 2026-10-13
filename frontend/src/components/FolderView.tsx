import { zipUrl } from '../api/docs';
import type { TreeNode } from '../api/types';
import { docUrl, formatDate, formatSize, openDoc } from '../store';
import { Breadcrumbs } from './Breadcrumbs';
import { Download, File, Folder } from './Icons';

/** The server writes a zip without zip64; a bigger directory is refused (`pack.rs`). */
const ZIP_MAX_FILES = 65_000;
const ZIP_MAX_BYTES = 4_000_000_000;

interface Shape {
  files: number;
  bytes: number;
}

function shape(node: TreeNode): Shape {
  if (!node.is_dir) return { files: 1, bytes: node.size ?? 0 };
  return node.children.reduce(
    (acc, c) => {
      const s = shape(c);
      return { files: acc.files + s.files, bytes: acc.bytes + s.bytes };
    },
    { files: 0, bytes: 0 },
  );
}

function plural(n: number, one: string, many: string): string {
  return `${n} ${n === 1 ? one : many}`;
}

function Row({ node }: { node: TreeNode }) {
  const s = node.is_dir ? shape(node) : null;
  return (
    <li class="folder-row">
      <a
        class="folder-link"
        href={docUrl(node.path)}
        onClick={(e) => {
          if (e.metaKey || e.ctrlKey || e.shiftKey || e.button !== 0) return;
          e.preventDefault();
          openDoc(node.path);
        }}
      >
        {node.is_dir ? <Folder class="folder-icon" size={16} /> : <File class="folder-icon" size={16} />}
        <span class="folder-name">
          <span class="folder-title">{node.is_dir ? node.name : node.title || node.name}</span>
          {!node.is_dir && node.title && node.title !== node.name && (
            <span class="folder-file muted">{node.name}</span>
          )}
          {node.summary && <span class="folder-summary muted">{node.summary}</span>}
        </span>
        <span class="folder-info muted">
          {s ? plural(s.files, 'file', 'files') : node.content_type}
        </span>
        <span class="folder-info muted">{formatSize(s ? s.bytes : (node.size ?? 0))}</span>
        <span class="folder-info folder-date muted">{node.updated_at ? formatDate(node.updated_at) : ''}</span>
      </a>
    </li>
  );
}

export function FolderView({ node }: { node: TreeNode }) {
  const s = shape(node);
  const zippable = s.files <= ZIP_MAX_FILES && s.bytes <= ZIP_MAX_BYTES;
  return (
    <article class="doc" key={node.path}>
      <header class="doc-header">
        <Breadcrumbs path={node.path} />
        <h1 class="doc-title">{node.name}</h1>
        <div class="doc-meta">
          <span class="pill">{plural(s.files, 'file', 'files')}</span>
          <span class="pill">{formatSize(s.bytes)}</span>
          {zippable ? (
            <a class="pill pill-link" href={zipUrl(node.path)} download title="Everything in this directory as one zip file">
              <Download size={13} /> Download .zip
            </a>
          ) : (
            <span class="pill" title="Too large for one zip file">no zip</span>
          )}
        </div>
      </header>
      <ul class="folder-list">
        {node.children.map((c) => (
          <Row key={c.path} node={c} />
        ))}
      </ul>
    </article>
  );
}
