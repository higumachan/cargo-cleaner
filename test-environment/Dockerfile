FROM rust:1-bookworm as builder

COPY ./ /app

WORKDIR /app
RUN cargo build --release
RUN apt install -y git
RUN mkdir /repos
WORKDIR /repos
RUN git clone https://github.com/rust-lang/cargo.git
RUN cd cargo && cargo build
RUN cargo new alice && cd alice && cargo build
RUN cargo new bob && cd bob && cargo build
RUN cargo new carol && cd carol && cargo build
RUN cargo new dave && cd dave && cargo build
RUN cargo new eve && cd eve && cargo build
RUN cargo new kuma && cd kuma && cargo build

FROM debian:bookworm-slim
RUN mkdir /app
COPY --from=builder /app/target/release/cargo-cleaner /app/cargo-cleaner
COPY --from=builder /repos/cargo /root/cargo
COPY --from=builder /repos/alice /root/alice
COPY --from=builder /repos/bob /root/bob
COPY --from=builder /repos/carol /root/carol
COPY --from=builder /repos/dave /root/dave
COPY --from=builder /repos/eve /root/eve
COPY --from=builder /repos/kuma /root/kuma
