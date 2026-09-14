/**
 * Source code. Highlighting is prepared up front, while the document is opened, so there is no
 * intermediate plain-text step here. Plain text remains for unsupported languages,
 * large files and the case where highlighting did not finish in time.
 */
export function CodeView({ code, html }: { code: string; html?: string | null }) {
  if (html) {
    return <div class="code-view code-view--numbered" dangerouslySetInnerHTML={{ __html: html }} />;
  }
  return <pre class="code"><code>{code}</code></pre>;
}
