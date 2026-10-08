/** Export to Google Drive from the browser. Google issues an hour-long access token to the page
 *  itself, so the file is created by the employee under their own account, and our server neither
 *  sees the token nor keeps any secret: the organization only names its OAuth client in
 *  GOOGLE_CLIENT_ID, and that identifier is public by design. */

/** The narrowest Drive scope: the app sees only the files it created itself. */
const SCOPE = 'https://www.googleapis.com/auth/drive.file';
const GIS_URL = 'https://accounts.google.com/gsi/client';
const UPLOAD_URL = 'https://www.googleapis.com/upload/drive/v3/files';

/** A file asked for in one of these types arrives in Drive as an editable Google one. */
export const GOOGLE_DOC = 'application/vnd.google-apps.document';
const GOOGLE_SHEET = 'application/vnd.google-apps.spreadsheet';
const GOOGLE_SLIDES = 'application/vnd.google-apps.presentation';

/** What a file turns into in Drive: a table becomes a Google table, an office document a Google
 *  one. A type that is not here is stored as the file itself. */
const CONVERT: Record<string, string> = {
  'text/csv': GOOGLE_SHEET,
  'text/tab-separated-values': GOOGLE_SHEET,
  'application/vnd.ms-excel': GOOGLE_SHEET,
  'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet': GOOGLE_SHEET,
  'application/vnd.oasis.opendocument.spreadsheet': GOOGLE_SHEET,
  'application/msword': GOOGLE_DOC,
  'application/vnd.openxmlformats-officedocument.wordprocessingml.document': GOOGLE_DOC,
  'application/vnd.oasis.opendocument.text': GOOGLE_DOC,
  'application/vnd.ms-powerpoint': GOOGLE_SLIDES,
  'application/vnd.openxmlformats-officedocument.presentationml.presentation': GOOGLE_SLIDES,
  'application/vnd.oasis.opendocument.presentation': GOOGLE_SLIDES,
};

export function googleTypeFor(contentType: string): string | undefined {
  return CONVERT[contentType];
}

interface TokenResponse {
  access_token?: string;
  expires_in?: number;
  error?: string;
}

interface TokenClient {
  requestAccessToken(): void;
}

interface TokenConfig {
  client_id: string;
  scope: string;
  callback: (response: TokenResponse) => void;
  error_callback?: (error: { type: string }) => void;
}

declare global {
  interface Window {
    google?: { accounts: { oauth2: { initTokenClient(config: TokenConfig): TokenClient } } };
  }
}

let loading: Promise<void> | undefined;

/** Google's script is pulled in only when the export is turned on: with GOOGLE_CLIENT_ID unset the
 *  page makes no request outside the contour. It is loaded before the click, because the sign-in
 *  window has to open while the browser still counts the click as a user action. */
export function loadGoogle(): Promise<void> {
  loading ??= new Promise<void>((resolve, reject) => {
    const el = document.createElement('script');
    el.src = GIS_URL;
    el.async = true;
    el.onload = () => resolve();
    el.onerror = () => {
      loading = undefined;
      el.remove();
      reject(new Error('Google sign-in did not load'));
    };
    document.head.append(el);
  });
  return loading;
}

let token: { value: string; expires: number } | undefined;

/** The implicit flow: there is no refresh token, an expired one is asked for again. */
async function accessToken(clientId: string): Promise<string> {
  if (token && token.expires - Date.now() > 60_000) return token.value;
  await loadGoogle();
  const oauth2 = window.google?.accounts.oauth2;
  if (!oauth2) throw new Error('Google sign-in is unavailable');

  return new Promise<string>((resolve, reject) => {
    oauth2
      .initTokenClient({
        client_id: clientId,
        scope: SCOPE,
        callback: (res) => {
          if (!res.access_token) return reject(new Error(res.error ?? 'access denied'));
          token = { value: res.access_token, expires: Date.now() + (res.expires_in ?? 3600) * 1000 };
          resolve(res.access_token);
        },
        error_callback: (err) => reject(new Error(err.type === 'popup_closed' ? 'window closed' : err.type)),
      })
      .requestAccessToken();
  });
}

async function driveError(res: Response): Promise<Error> {
  // A rejected token is dropped so that the next export asks Google again
  if (res.status === 401) token = undefined;
  const text = await res.text().catch(() => '');
  try {
    return new Error(JSON.parse(text).error?.message ?? `HTTP ${res.status}`);
  } catch {
    return new Error(`HTTP ${res.status}`);
  }
}

export interface DriveUpload {
  /** File name in Drive. */
  name: string;
  body: Blob;
  /** Type of the body as it leaves us. */
  sourceType: string;
  /** Google type to convert into; without it the file is stored as is. */
  targetType?: string;
}

/** Multipart upload: metadata and the body in one request. The body stays a Blob and is not read
 *  into a string, so a large file does not land in memory twice. */
export async function uploadToDrive(clientId: string, file: DriveUpload): Promise<string> {
  const bearer = await accessToken(clientId);
  const meta: Record<string, string> = { name: file.name };
  if (file.targetType) meta.mimeType = file.targetType;

  const boundary = `adocs-${crypto.randomUUID()}`;
  const body = new Blob([
    `--${boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n${JSON.stringify(meta)}\r\n`,
    `--${boundary}\r\nContent-Type: ${file.sourceType}\r\n\r\n`,
    file.body,
    `\r\n--${boundary}--\r\n`,
  ]);

  const res = await fetch(`${UPLOAD_URL}?uploadType=multipart&fields=webViewLink`, {
    method: 'POST',
    headers: { Authorization: `Bearer ${bearer}`, 'Content-Type': `multipart/related; boundary=${boundary}` },
    body,
  });
  if (!res.ok) throw await driveError(res);
  const created = (await res.json()) as { webViewLink?: string };
  return created.webViewLink ?? 'https://drive.google.com/';
}

/** The name the file gets in Drive: a converted one loses its extension, the rest keep theirs. */
export function driveName(path: string, converted: boolean): string {
  const file = path.split('/').pop() ?? path;
  return converted ? file.replace(/\.[^.]+$/, '') : file;
}
