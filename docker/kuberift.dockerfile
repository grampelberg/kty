FROM lukemathwalker/cargo-chef:latest-rust-slim-bookworm as chef
WORKDIR /app
RUN apt-get update && apt-get install -y \
    libssl-dev \
    pkg-config

FROM chef as planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef as builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release --bin kuberift

FROM debian:bookworm-slim as runtime
WORKDIR /app
COPY --from=builder /app/target/release/kuberift /usr/local/bin
CMD ["/usr/local/bin/kuberift", "serve", "-vv"]
