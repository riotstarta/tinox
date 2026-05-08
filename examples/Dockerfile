FROM rust:slim AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    clang llvm && rm -rf /var/lib/apt/lists/*

WORKDIR /tinox
COPY . .
RUN cargo build --release --bin tinox

RUN ./target/release/tinox build examples/simple_test.tnx -o /app


FROM debian:bookworm-slim

COPY --from=builder /app /app

ENTRYPOINT ["/app"]
