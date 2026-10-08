import { useEffect } from 'preact/hooks';
import { Check, Close, ExternalLink, Refresh } from './Icons';

export interface DriveState {
  busy: boolean;
  /** Link to the finished file in Drive. */
  link?: string;
  error?: string;
}

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

  return (
    <div class="drive-backdrop">
      <div class="drive-dialog" role="dialog" aria-modal="true" aria-label="Export to Google Drive">
        {state.busy && (
          <>
            <div class="drive-icon"><Refresh size={22} class="drive-spin" /></div>
            <h2>Saving to Google Drive</h2>
            <p class="drive-file">{name}</p>
            <p class="drive-note">A window of Google's own may ask you to sign in and allow access. The file lands in your own Drive.</p>
            <div class="drive-actions">
              <button type="button" class="btn btn-secondary btn-sm" onClick={onClose}>Close</button>
            </div>
          </>
        )}

        {!state.busy && state.link && (
          <>
            <div class="drive-icon drive-icon--ok"><Check size={22} /></div>
            <h2>Saved to Google Drive</h2>
            <p class="drive-file">{name}</p>
            <div class="drive-actions">
              <a class="btn btn-primary btn-sm" href={state.link} target="_blank" rel="noopener">
                <ExternalLink size={14} /> Open the file
              </a>
              <button type="button" class="btn btn-secondary btn-sm" onClick={onClose}>Close</button>
            </div>
          </>
        )}

        {!state.busy && !state.link && (
          <>
            <div class="drive-icon drive-icon--error"><Close size={22} /></div>
            <h2>Google Drive did not take the file</h2>
            <p class="message message-error drive-message">{state.error}</p>
            <div class="drive-actions">
              <button type="button" class="btn btn-primary btn-sm" onClick={onRetry}>Try again</button>
              <button type="button" class="btn btn-secondary btn-sm" onClick={onClose}>Close</button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
