import { render } from 'preact';
import { useEffect } from 'preact/hooks';
import '../css/style.css';
import '../css/docs.css';
import { Nav } from '../components/Nav';
import { Tree } from '../components/Tree';
import { DocViewer } from '../components/DocViewer';
import { SearchDialog } from '../components/SearchDialog';
import { Expand, PanelLeft, Search, Shrink } from '../components/Icons';
import {
  build, closePeek, closePeekSoon, currentPath, filter, loadBuild, loadTree, narrow, openDoc, openPeek,
  pathFromLocation, peek, syncWithServer, toggleSidebar, toggleWide, tree, treeError, treeHidden, wide,
} from '../store';

function SidebarToggle() {
  const hidden = treeHidden.value;
  return (
    <button
      type="button"
      class={`icon-btn nav-toggle ${hidden ? '' : 'icon-btn--on'}`}
      title={hidden ? 'Show the tree' : 'Collapse the tree'}
      aria-label="Document tree"
      onMouseEnter={() => !narrow.value && openPeek()}
      onMouseLeave={closePeekSoon}
      onClick={toggleSidebar}
    >
      <PanelLeft size={18} />
    </button>
  );
}

function DocsPage() {
  useEffect(() => {
    loadTree();
    if (currentPath.value) {
      loadBuild();
      openDoc(currentPath.value, false);
    } else {
      // Like a repository page: with no document selected the root README is opened.
      loadBuild().then(() => {
        const readme = build.value?.readme;
        if (readme && !currentPath.value) openDoc(readme, false);
      });
    }

    const onPop = () => {
      const p = pathFromLocation();
      if (p) openDoc(p, false);
      else currentPath.value = '';
    };
    window.addEventListener('popstate', onPop);

    const onVisible = () => {
      if (document.visibilityState === 'visible') syncWithServer();
    };
    document.addEventListener('visibilitychange', onVisible);

    return () => {
      window.removeEventListener('popstate', onPop);
      document.removeEventListener('visibilitychange', onVisible);
    };
  }, []);

  const hidden = treeHidden.value;
  const showPeek = hidden && peek.value;
  const layoutClass = [
    'docs-layout',
    hidden ? 'docs-layout--collapsed' : '',
    showPeek ? 'docs-layout--peek' : '',
    wide.value ? 'docs-layout--wide' : '',
  ].join(' ');

  function open(path: string) {
    closePeek();
    openDoc(path);
  }

  return (
    <>
      <Nav left={<SidebarToggle />} />
      <div class={layoutClass}>
        <div class="sidebar-slot" />
        {showPeek && <div class="sidebar-backdrop" onClick={closePeek} />}
        <aside class="sidebar" onMouseEnter={openPeek} onMouseLeave={closePeekSoon}>
          <label class="sidebar-search">
            <Search size={15} />
            <input
              placeholder="Search by name"
              value={filter.value}
              onInput={(e) => (filter.value = (e.target as HTMLInputElement).value)}
            />
          </label>
          <div class="sidebar-tree">
            {treeError.value && <p class="message message-error">{treeError.value}</p>}
            {tree.value ? <Tree root={tree.value} onOpen={open} /> : <p class="muted tree-empty">Loading...</p>}
          </div>
        </aside>
        <main class="content">
          <div class="content-toolbar">
            <button
              type="button"
              class={`icon-btn ${wide.value ? 'icon-btn--on' : ''}`}
              title={wide.value ? 'Normal width' : 'Full width'}
              onClick={toggleWide}
            >
              {wide.value ? <Shrink size={17} /> : <Expand size={17} />}
            </button>
          </div>
          <div class="content-inner">
            <DocViewer />
          </div>
        </main>
      </div>
      <SearchDialog />
    </>
  );
}

// The server puts a note for clients without JavaScript into the page; people do not need it.
document.getElementById('agent-note')?.remove();
render(<DocsPage />, document.body);
