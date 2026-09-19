FROM rust:1.98-bookworm AS builder
WORKDIR /source
COPY . .
RUN cargo build --locked --release -p verbsmith-server

FROM debian:bookworm-slim
RUN useradd --system --uid 10001 --create-home verbsmith
COPY --from=builder /source/target/release/verbsmith-server /usr/local/bin/verbsmith-server
USER verbsmith
WORKDIR /home/verbsmith
EXPOSE 8787
ENTRYPOINT ["verbsmith-server"]

