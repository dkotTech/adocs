# Deploying adocs

Guidelines for wiring adocs into a project. Adapt them to the project's CI and infrastructure;
the variables are listed in `README.md`.

## Model

- The adocs binary and image are generic. Never build adocs per project; take a pinned release
  (`adocs-linux-x86_64` + `SHA256SUMS`, or the `<user>/adocs:<version>` image).
- The content is one archive of the repository: `git archive --format=tar.gz --prefix=repo/ -o repo.tar.gz HEAD`.
  Tracked files only; exclude extras with `export-ignore` in `.gitattributes`. The common top
  directory is stripped.
- The service reads `$ADOCS_DATA_DIR/$ADOCS_ARCHIVE` at startup and again on `POST /api/refresh`.
  Nothing else re-reads it. A missing or broken archive keeps the previous version serving.

## Choose a delivery

1. **Mounted archive (default).** One long-running adocs; CI replaces the archive on a shared volume
   (compose volume, PVC, S3 via a CSI/FUSE driver) and then calls `POST /api/refresh`. No redeploy per
   docs change.
2. **Archive baked into an image.** When the platform only deploys images:
   ```dockerfile
   FROM <user>/adocs:<version>
   COPY repo.tar.gz /data/repo.tar.gz
   ```
   The official image already sets `ADOCS_DATA_DIR=/data`, `ADOCS_ARCHIVE=repo.tar.gz`,
   `APP_HOST=0.0.0.0`, `APP_PORT=8080`. Every docs change is a new image and a rollout.
3. **Binary without Docker.** Download the release binary, verify it against `SHA256SUMS`, run it
   with a `.env` next to it (`ADOCS_DATA_DIR`, `ADOCS_ARCHIVE`, `APP_HOST`). Suits a VM or systemd.

## Rules

- **Replace the archive atomically**: write to a temporary name in the same directory, then rename.
  Never write in place: a refresh could read half a file.
- **Refresh every replica.** `POST /api/refresh` affects only the replica that receives it. With
  several replicas, call each pod directly, or restart them (rollout), or run a single replica.
- **Mount the data read-only**; the cache (`ADOCS_CACHE_DIR`) must be writable, local and not shared
  (`emptyDir`, container tmp). It holds the extracted archive, so size it for the unpacked content.
- **Set `ADOCS_MAX_SIZE_MB`** above the unpacked repository size, or the refresh fails.
- **Restrict access outside adocs.** There is no auth; reading and `POST /api/refresh` are public.
  Put it behind the organization's VPN, SSO proxy or ingress auth.
- **Set `APP_BASE_URL`** to the public URL, so `llms.txt`, the sitemap and agent links are absolute and
  correct. Use `APP_TRUST_FORWARDED=true` only behind a proxy that overwrites `X-Forwarded-*`.
- **Set `APP_ALLOWED_HOSTS`** to the public host name(s) when the docs are sensitive (DNS rebinding).
  Health checks on `/pub/health` are exempt, so probes by pod IP keep working.
- **Probes**: liveness and readiness on `GET /pub/health`. It answers even without an archive; check
  `GET /api/build` to confirm the archive is loaded.
- **Pin versions** of the adocs image or binary in CI, so docs builds are reproducible.
- **Do not cache HTML aggressively** in a CDN: pages and indexes vary by `Accept` and use ETags with
  `no-cache`. `/raw/*` is safe to cache with revalidation.

## Minimal CI step (mounted archive)

```bash
git archive --format=tar.gz --prefix=repo/ -o repo.tar.gz HEAD
cp repo.tar.gz /data/.repo.tar.gz.tmp && mv /data/.repo.tar.gz.tmp /data/repo.tar.gz
curl -fsS -X POST https://docs.example.com/api/refresh
```

## Check after deploy

- `GET /pub/health` → `{"status":"ok"}`.
- `GET /api/build` shows the expected archive and refresh time, no error.
- `GET /llms.txt` lists the files with absolute links on the public host.
- For agents: `claude mcp add --transport http adocs https://docs.example.com/mcp`.
