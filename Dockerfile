FROM node:22-bookworm-slim AS frontend
WORKDIR /build/web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

FROM rust:1.99-bookworm AS backend
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src/ ./src/
COPY profiles/ ./profiles/
RUN cargo build --locked --release

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 zako \
    && useradd --uid 10001 --gid zako --no-create-home --shell /usr/sbin/nologin zako \
    && mkdir -p /app/web/dist /app/licenses /data \
    && chown -R zako:zako /app /data
WORKDIR /app
COPY --from=backend --chown=zako:zako /build/target/release/zakoai-proxy /app/zakoai-proxy
COPY --from=frontend --chown=zako:zako /build/web/dist/ /app/web/dist/
COPY --chown=zako:zako LICENSE third_party/gpt-instruct.LICENSE /app/licenses/
ENV DATA_DIR=/data \
    WEB_DIR=/app/web/dist \
    BIND_ADDR=0.0.0.0:1990 \
    RUST_LOG=zakoai_proxy=info
USER 10001:10001
VOLUME ["/data"]
EXPOSE 1990
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
    CMD curl --fail --silent --show-error http://127.0.0.1:1990/health || exit 1
ENTRYPOINT ["/app/zakoai-proxy"]
