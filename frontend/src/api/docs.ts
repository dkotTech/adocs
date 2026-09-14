import { apiFetch } from './client';
import type { BuildInfo, DocResponse, RefreshResponse, SearchResponse, TreeNode } from './types';

export function encodePath(path: string): string {
  return path.split('/').map(encodeURIComponent).join('/');
}

export function rawUrl(path: string): string {
  return `/raw/${encodePath(path)}`;
}

export function getTree(): Promise<TreeNode> {
  return apiFetch<TreeNode>('/api/tree');
}

export function getDoc(path: string): Promise<DocResponse> {
  return apiFetch<DocResponse>(`/api/docs/${encodePath(path)}`);
}

export function getBuild(): Promise<BuildInfo> {
  return apiFetch<BuildInfo>('/api/build');
}

export function postRefresh(): Promise<RefreshResponse> {
  return apiFetch<RefreshResponse>('/api/refresh', { method: 'POST' });
}

export interface SearchParams {
  query: string;
  regex: boolean;
  caseSensitive: boolean;
}

/** Results for the dialog: more lines than the API default, no context lines. */
export function searchDocs(p: SearchParams, signal?: AbortSignal): Promise<SearchResponse> {
  const qs = new URLSearchParams({ q: p.query, case: p.caseSensitive ? 'sensitive' : 'smart', limit: '300' });
  if (p.regex) qs.set('regex', 'true');
  return apiFetch<SearchResponse>(`/api/search?${qs}`, { signal });
}
