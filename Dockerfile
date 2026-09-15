# One recipe for every service: docker build --build-arg SERVICE=auth-service .
FROM rust:1.96-slim-bookworm AS build
ARG SERVICE
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p "${SERVICE}" \
    && cp "target/release/${SERVICE}" /usr/local/bin/app

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /usr/local/bin/app /usr/local/bin/app
USER 10001
ENTRYPOINT ["/usr/local/bin/app"]
