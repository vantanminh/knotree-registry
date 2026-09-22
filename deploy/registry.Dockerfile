FROM node:22-bookworm-slim AS web-build
WORKDIR /web
COPY web/package.json web/package-lock.json ./
RUN npm ci --ignore-scripts
COPY web/ ./
RUN npm run build

FROM rust:1.97-bookworm AS build
WORKDIR /src
COPY . .
COPY --from=web-build /web/dist /src/web/dist
RUN cargo build --release -p registry-server

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl && rm -rf /var/lib/apt/lists/*
RUN useradd --system --create-home --uid 10001 registry
COPY --from=build /src/target/release/registry-server /usr/local/bin/registry-server
COPY --from=web-build /web/dist /opt/knotree-registry/web
RUN mkdir -p /var/lib/knotree-registry && chown -R registry:registry /var/lib/knotree-registry
USER registry
ENV APP_ENV=production
ENV STORAGE_ROOT=/var/lib/knotree-registry/objects
ENV STATIC_ROOT=/opt/knotree-registry/web
EXPOSE 8080
HEALTHCHECK --interval=15s --timeout=5s --start-period=10s --retries=5 CMD ["curl", "--fail", "--silent", "http://127.0.0.1:8080/livez"]
ENTRYPOINT ["/usr/local/bin/registry-server"]
