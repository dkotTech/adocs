# adocs

Shows a repository as documentation. The binary does not depend on the content: CI puts a repository
archive into a shared directory, the service reads it, and the "Refresh" button loads a new version.

## How it works

- The archive (tar, tar.gz or zip) lives in a mounted directory: a docker-compose volume, a Kubernetes
  persistent volume or S3 via a driver.
- It is copied to a local cache and extracted into a version directory. Only the index is kept in
  memory, files are streamed. A broken or missing archive keeps the previous version serving.
- "Refresh" does nothing if the archive's size and date or its sha256 are unchanged; otherwise the new
  version replaces the old one without downtime. The refresh time is shown in the header.
- Replicas refresh independently: the button affects the replica that got the request.

## Configuration

| Variable | Default | Meaning |
|---|---|---|
| `ADOCS_DATA_DIR` | required | mounted directory with the archive |
| `ADOCS_ARCHIVE` | required | archive file name inside it |
| `ADOCS_CACHE_DIR` | temp dir | local cache |
| `ADOCS_MAX_SIZE_MB` | `1024` | limit for the archive and its extracted content |
| `ADOCS_SEARCH_REGEX` | `false` | allow regular expressions in search |
| `APP_HOST`, `APP_PORT` | `127.0.0.1`, `8080` | listen address |
| `APP_BASE_URL` | from `Host` | public address for absolute links; set it in production |
| `APP_TRUST_FORWARDED` | `false` | take that address from `X-Forwarded-*`, only behind a proxy that sets them |
| `APP_ALLOWED_HOSTS` | off | answer only to these host names, `421` otherwise (DNS rebinding protection) |
| `SITE_TITLE` | `adocs` | title in the header and in `llms.txt` |

Values with spaces in `.env` must be quoted.

## CI

```bash
git archive --format=tar.gz --prefix=repo/ -o repo.tar.gz HEAD
cp repo.tar.gz /data/.repo.tar.gz.tmp && mv /data/.repo.tar.gz.tmp /data/repo.tar.gz
```

Replace the archive atomically (rename on a volume; S3 uploads already are). `examples/ci` has ready
pipelines for GitHub Actions and Bitbucket: they build an image with the archive inside and a bundle
with the release binary, the archive and `.env`.

## Deployment

```yaml
# docker-compose
services:
  adocs:
    image: adocs
    ports: ["8080:8080"]
    environment:
      ADOCS_ARCHIVE: repo.tar.gz
    volumes:
      - ./data:/data:ro
```

```yaml
# Kubernetes: archive on a persistent volume, cache in emptyDir
containers:
  - name: adocs
    image: adocs
    env:
      - { name: ADOCS_DATA_DIR, value: /data }
      - { name: ADOCS_ARCHIVE, value: repo.tar.gz }
      - { name: ADOCS_CACHE_DIR, value: /cache }
    volumeMounts:
      - { name: docs, mountPath: /data, readOnly: true }
      - { name: cache, mountPath: /cache }
volumes:
  - name: docs
    persistentVolumeClaim: { claimName: docs, readOnly: true }
  - name: cache
    emptyDir: {}
```

Everything is public, including `POST /api/refresh`: restrict access from the outside.

## Addresses

| URL | What |
|---|---|
| `/`, `/docs/<path>` | web page; the file itself (or `llms.txt` at `/`) for clients without `text/html` in `Accept` |
| `/raw/<path>` | file as is, with `ETag` and `Range` |
| `/llms.txt`, `/robots.txt`, `/sitemap.xml` | entry points for agents and crawlers |
| `/api/tree`, `/api/docs/<path>` | tree and document metadata as JSON |
| `/api/search?q=<text>` | full-text search |
| `/api/build`, `POST /api/refresh` | archive state and refresh |
| `/mcp` | MCP server |
| `/pub/health` | liveness check, exempt from the host check |

All reading addresses answer `HEAD`. Generated indexes carry an `ETag` and answer `304`.

## AI agents

- `llms.txt` lists every file with a summary: the first descriptive paragraph of a markdown file
  (not a list, not a navigation line, at least 30 characters).
- A browser page for an agent that sends `text/html` still contains a link to the raw file and to
  `llms.txt`.
- MCP over Streamable HTTP, without sessions:

```bash
claude mcp add --transport http adocs https://docs.example.com/mcp
```

| Tool | What it does |
|---|---|
| `list_docs(dir?)` | files with titles and summaries |
| `read_doc(path, offset?, limit?)` | text file by numbered lines, 500 by default, up to 2000; refuses binary files |
| `search(q, path?, context?, limit?)` | same as `/api/search`, `path:line:text` output |

## Search

In the interface, results open in a dialog from 2 typed characters; a click opens the document at the
match. Search runs on the ripgrep libraries over the extracted files, text files only. Files without an
extension (`Dockerfile`, `Makefile`) are not searched.

| Parameter | Meaning |
|---|---|
| `q` | query, 2–1000 characters, smart case |
| `regex=true` | regular expression, needs `ADOCS_SEARCH_REGEX=true` |
| `case` | `smart`, `sensitive` or `insensitive` |
| `path` | only this file or directory |
| `context` | lines around each match, up to 5 |
| `limit`, `per_file` | matching lines in total (100, up to 1000) and per file (20) |
| `format` | `json` or `text` (`path:line:text`); `Accept: text/plain` also gives text |

Limits: 4 searches at once, others wait up to 2 s and then get `429`; a search stops after 5 s with
`truncated: true`; lines are cut to 400 characters.

## Security

The archive is untrusted: `..` and absolute paths are dropped, links and devices skipped, sizes limited
by `ADOCS_MAX_SIZE_MB`. Hidden files are not shown.

## Build

```bash
cargo build --release -p server    # target/release/adocs
docker build -t adocs .
```

`build.rs` builds the frontend; with a ready `frontend/dist` and no npm, set `SKIP_FRONTEND_BUILD=1`.

Every push to `master` publishes a GitHub release (`.github/workflows/release.yml`): the next patch after
the last `vX.Y.Z` tag, starting with `v0.1.0`, with `adocs-linux-x86_64` and `SHA256SUMS`. With the
`DOCKERHUB_USERNAME` and `DOCKERHUB_TOKEN` secrets the image is also pushed as `<user>/adocs:<version>`
and `latest`. For a minor or major version, push its tag by hand (`git tag v0.2.0 && git push origin v0.2.0`):
that tag is released, and later pushes to `master` continue from it.
