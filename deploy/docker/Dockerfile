# Hardened `weave-registry` image: distroless, non-root, read-only-rootfs
# compatible — all state lives in the mounted DATA_DIR volume. Build from
# the workspace root:
#   docker build -f deploy/docker/Dockerfile -t weave-registry:latest .
FROM rust:1.93-slim AS build
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
# Only the hub crate (and its workspace deps) is needed for this image.
RUN cargo build --release -p weave-graph-hub --bin weave-registry

FROM gcr.io/distroless/cc-debian12
COPY --from=build /build/target/release/weave-registry /usr/local/bin/weave-registry
# Non-root: the registry only needs write access to its data volume.
USER 65532:65532
VOLUME ["/data"]
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/weave-registry"]
# 0.0.0.0 binds every interface *inside this container's own network
# namespace* — safe only because the port is reachable exclusively through
# whatever published it (a compose `expose`, a k8s Service, `docker run -p`).
CMD ["--bind", "0.0.0.0:8080", "--data-dir", "/data", \
     "--max-queue-depth-per-repo", "1000", \
     "--max-pushes-per-minute-per-repo", "600"]
