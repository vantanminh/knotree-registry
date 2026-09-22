FROM rust:1.97-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release -p registry-server

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
RUN useradd --system --create-home --uid 10001 registry
COPY --from=build /src/target/release/registry-server /usr/local/bin/registry-server
RUN mkdir -p /var/lib/knotree-registry && chown -R registry:registry /var/lib/knotree-registry
USER registry
ENV STORAGE_ROOT=/var/lib/knotree-registry/objects
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/registry-server"]
