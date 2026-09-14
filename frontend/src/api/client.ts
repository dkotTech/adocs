import { API_URL } from '../config';

export class ApiError extends Error {
  constructor(public status: number, message: string) {
    super(message);
  }
}

async function throwIfNotOk(resp: Response): Promise<void> {
  if (resp.ok) return;
  let message = `HTTP ${resp.status}`;
  try {
    const body = await resp.json();
    if (body?.error) message = body.error;
  } catch { /* not json */ }
  throw new ApiError(resp.status, message);
}

export async function apiFetch<T>(path: string, options: RequestInit = {}): Promise<T> {
  const resp = await fetch(`${API_URL}${path}`, {
    ...options,
    credentials: 'include',
    headers: {
      'Content-Type': 'application/json',
      ...(options.headers ?? {}),
    },
  });
  await throwIfNotOk(resp);
  const len = resp.headers.get('content-length');
  if (resp.status === 204 || len === '0') return null as T;
  return resp.json() as Promise<T>;
}

/** Uploading a file as is: the request body is the file itself. */
export async function apiUpload<T>(path: string, file: Blob, contentType?: string): Promise<T> {
  const resp = await fetch(`${API_URL}${path}`, {
    method: 'PUT',
    credentials: 'include',
    headers: contentType ? { 'Content-Type': contentType } : {},
    body: file,
  });
  await throwIfNotOk(resp);
  return resp.json() as Promise<T>;
}
