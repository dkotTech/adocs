export interface TreeNode {
  name: string;
  path: string;
  is_dir: boolean;
  title?: string;
  /** The first paragraph of a markdown file. */
  summary?: string;
  content_type?: string;
  size?: number;
  updated_at?: string;
  children: TreeNode[];
}

export interface DocMeta {
  path: string;
  title: string;
  /** The first paragraph of a markdown file. */
  summary?: string;
  content_type: string;
  size: number;
  etag: string;
  updated_at?: string;
}

export type Render =
  | { kind: 'html'; body: string }
  | { kind: 'text'; body: string }
  | { kind: 'frame' }
  | { kind: 'pdf' }
  | { kind: 'image' }
  | { kind: 'binary' };

export interface DocResponse {
  meta: DocMeta;
  render: Render;
  /** Highlighted HTML of the source, prepared on the client before the document is shown. */
  highlighted?: string | null;
  /** Opened from a search hit: the page scrolls to the marked match. */
  jump?: boolean;
}

/** Archive state, shared by every user. */
export interface BuildInfo {
  archive: string;
  hash: string | null;
  archive_modified_at: string | null;
  updated_at: string | null;
  checked_at: string | null;
  documents: number;
  total_size: number;
  readme: string | null;
  error: string | null;
  /** The server accepts regular expressions in search. */
  search_regex: boolean;
}

export interface RefreshResponse {
  result: 'updated' | 'unchanged' | 'busy' | 'error';
  message: string;
  build: BuildInfo;
}

export interface SearchLine {
  line: number;
  text: string;
}

export interface SearchHit extends SearchLine {
  /** Match positions in `text` as character offsets, [start, end). */
  ranges: [number, number][];
  before?: SearchLine[];
  after?: SearchLine[];
}

export interface SearchFile {
  path: string;
  title: string;
  /** Matching lines in the file, including those not returned. */
  count: number;
  matches: SearchHit[];
}

export interface SearchResponse {
  files: SearchFile[];
  total_matches: number;
  files_searched: number;
  truncated: boolean;
  elapsed_ms: number;
}
