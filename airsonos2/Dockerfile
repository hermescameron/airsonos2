FROM rust:1.88-bookworm AS builder
WORKDIR /src

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY vendor ./vendor
RUN cargo build --release --locked --bin airsonos2

FROM ghcr.io/home-assistant/base-debian:bookworm

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates ffmpeg \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /src/target/release/airsonos2 /usr/local/bin/airsonos2
COPY airsonos2/run.sh /run.sh
RUN chmod a+x /run.sh

CMD ["/run.sh"]
