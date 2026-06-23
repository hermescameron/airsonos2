FROM rust:1.86-bookworm AS builder
WORKDIR /src

COPY Cargo.toml Cargo.lock* ./
COPY crates ./crates
RUN cargo build --release --locked --bin airsonos2

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates ffmpeg \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /src/target/release/airsonos2 /usr/local/bin/airsonos2
COPY docs/config.example.toml /etc/airsonos2/config.toml

VOLUME ["/var/lib/airsonos2"]
EXPOSE 5000-5010/tcp 7000/tcp 9100/tcp
ENTRYPOINT ["/usr/local/bin/airsonos2"]
CMD ["serve", "--config", "/etc/airsonos2/config.toml"]
