# The image is built once and does not depend on the documentation.
# The archive lives in a mounted directory, the program copies it into a local cache.
#   docker run -p 8080:8080 -v "$PWD/data:/data:ro" -e ADOCS_ARCHIVE=repo.tar.gz adocs

FROM node:22-alpine AS frontend
WORKDIR /app/frontend
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend/ ./
RUN npm run build

FROM rust:1-alpine AS backend
RUN apk add --no-cache musl-dev
WORKDIR /app
# The frontend is built by the previous stage, there is no npm here.
ENV SKIP_FRONTEND_BUILD=1
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
COPY --from=frontend /app/frontend/dist/ frontend/dist/
# A static musl build: the same binary works when downloaded from a release.
RUN cargo build --release -p server

FROM alpine:3
RUN adduser -D -u 10001 adocs && mkdir -p /tmp/adocs && chown adocs /tmp/adocs
COPY --from=backend /app/target/release/adocs /usr/local/bin/adocs
USER adocs
ENV APP_HOST=0.0.0.0 \
    APP_PORT=8080 \
    ADOCS_DATA_DIR=/data \
    ADOCS_ARCHIVE=repo.tar.gz \
    ADOCS_CACHE_DIR=/tmp/adocs
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/adocs"]
