import type { TreeNode } from '../api/types';
import { currentPath, docUrl, expanded, filter, toggleDir } from '../store';
import { ChevronRight, File, Folder } from './Icons';

interface Props {
  root: TreeNode;
  onOpen: (path: string) => void;
}

function matches(node: TreeNode, q: string): boolean {
  if (!q) return true;
  if (node.is_dir) return node.children.some((c) => matches(c, q));
  return node.path.toLowerCase().includes(q) || (node.title ?? '').toLowerCase().includes(q);
}

export function Tree({ root, onOpen }: Props) {
  const q = filter.value.trim().toLowerCase();
  const visible = root.children.filter((c) => matches(c, q));
  if (visible.length === 0) {
    return <p class="tree-empty">{q ? 'Nothing found' : 'No documents yet'}</p>;
  }
  return (
    <ul class="tree" role="tree">
      {visible.map((c) => (
        <TreeItem key={c.path} node={c} q={q} depth={0} onOpen={onOpen} />
      ))}
    </ul>
  );
}

interface ItemProps {
  node: TreeNode;
  q: string;
  depth: number;
  onOpen: (path: string) => void;
}

function TreeItem({ node, q, depth, onOpen }: ItemProps) {
  const indent = { paddingLeft: `${8 + depth * 14}px` };

  if (node.is_dir) {
    const open = q !== '' || expanded.value.has(node.path);
    return (
      <li class="tree-dir" role="treeitem" aria-expanded={open}>
        <button type="button" class="tree-row" style={indent} onClick={() => toggleDir(node.path)}>
          <ChevronRight class={`tree-arrow ${open ? 'tree-arrow--open' : ''}`} size={14} />
          <Folder class="tree-icon" size={15} />
          <span class="tree-name">{node.name}</span>
        </button>
        <div class={`tree-children ${open ? 'tree-children--open' : ''}`}>
          <ul class="tree" role="group">
            {node.children
              .filter((c) => matches(c, q))
              .map((c) => (
                <TreeItem key={c.path} node={c} q={q} depth={depth + 1} onOpen={onOpen} />
              ))}
          </ul>
        </div>
      </li>
    );
  }

  const active = currentPath.value === node.path;
  return (
    <li class="tree-doc" role="treeitem" aria-selected={active}>
      {/* The whole row is clickable, not just the link text */}
      <a
        class={`tree-row tree-link ${active ? 'tree-row--active' : ''}`}
        style={indent}
        href={docUrl(node.path)}
        title={node.path}
        onClick={(e) => {
          if (e.metaKey || e.ctrlKey || e.shiftKey || e.button !== 0) return;
          e.preventDefault();
          onOpen(node.path);
        }}
      >
        <span class="tree-arrow tree-arrow--placeholder" />
        <File class="tree-icon" size={15} />
        <span class="tree-name">{node.title || node.name}</span>
      </a>
    </li>
  );
}
