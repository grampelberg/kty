FROM lukemathwalker/cargo-chef:latest-rust-slim-bookworm AS chef
WORKDIR /app
RUN apt-get update && apt-get install -y wget gpg lsb-release && \
    wget -qO - 'https://proget.makedeb.org/debian-feeds/prebuilt-mpr.pub' \
    | gpg --dearmor | tee /usr/share/keyrings/prebuilt-mpr-archive-keyring.gpg 1> /dev/null && \
    echo "deb [arch=all,$(dpkg --print-architecture) signed-by=/usr/share/keyrings/prebuilt-mpr-archive-keyring.gpg] https://proget.makedeb.org prebuilt-mpr $(lsb_release -cs)" \
    | tee /etc/apt/sources.list.d/prebuilt-mpr.list
RUN apt-get update && apt-get install -y \
    git \
    just \
    libssl-dev \
    pkg-config

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN just build-binary

FROM debian:bookworm-slim AS runtime
WORKDIR /app
RUN apt-get update && apt-get install -y \
    libssl3 \
    ca-certificates \
    && \
    apt-get clean

COPY --from=builder /app/target/release/kuberift /usr/local/bin
CMD ["/usr/local/bin/kuberift", "serve", "-vv"]
