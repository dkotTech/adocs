import { useEffect } from 'preact/hooks';
import { Check, Close, ExternalLink, Refresh } from './Icons';

/** Uploading, done with a link to the file in Drive, or failed. */
export type DriveState = { busy: true } | { link: string } | { error: string };

/** What is happening with the export to Google Drive. The upload goes through a window of Google's
 *  own and takes a while, so the page says it out loud instead of leaving a quiet button.
 *  A click past the dialog does not close it, only "Close" and Escape do: the upload is easy to
 *  dismiss by accident while the attention is in Google's window. Closing it during the upload only
 *  hides it, and the finished result shows up again. */
export function DriveDialog({
  name,
  state,
  onClose,
  onRetry,
}: {
  name: string;
  state: DriveState;
  onClose: () => void;
  onRetry: () => void;
}) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  let icon, title, body, primary;
  if ('link' in state) {
    icon = <div class="drive-icon drive-icon--ok"><Check size={22} /></div>;
    title = 'Saved to Google Drive';
    body = <p class="drive-file">{name}</p>;
    primary = (
      <a class="btn btn-primary btn-sm" href={state.link} target="_blank" rel="noopener">
        <ExternalLink size={14} /> Open the file
      </a>
    );
  } else if ('error' in state) {
    icon = <div class="drive-icon drive-icon--error"><Close size={22} /></div>;
    title = 'Google Drive did not take the file';
    body = <p class="message message-error drive-message">{state.error}</p>;
    primary = <button type="button" class="btn btn-primary btn-sm" onClick={onRetry}>Try again</button>;
  } else {
    icon = <div class="drive-icon"><Refresh size={22} class="spin" /></div>;
    title = 'Saving to Google Drive';
    body = (
      <>
        <p class="drive-file">{name}</p>
        <p class="drive-note">A window of Google's own may ask you to sign in and allow access. The file lands in your own Drive.</p>
      </>
    );
  }

  return (
    <div class="drive-backdrop">
      <div class="drive-dialog" role="dialog" aria-modal="true" aria-label="Export to Google Drive">
        {icon}
        <h2>{title}</h2>
        {body}
        <div class="drive-actions">
          {primary}
          <button type="button" class="btn btn-secondary btn-sm" onClick={onClose}>Close</button>
        </div>
      </div>
    </div>
  );
}
