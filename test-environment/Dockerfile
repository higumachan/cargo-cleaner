FROM rust:1-bookworm as builder

RUN mkdir /repos
WORKDIR /repos
RUN apt install -y git
RUN git clone https://github.com/rust-lang/cargo.git
RUN cd cargo && cargo build
RUN cargo new alice && cd alice && cargo build
RUN cargo new bob && cd bob && cargo build
RUN cargo new carol && cd carol && cargo build
RUN cargo new dave && cd dave && cargo build
RUN cargo new eve && cd eve && cargo build
RUN cargo new kuma && cd kuma && cargo build
COPY ./ /app
WORKDIR /app
RUN cargo build --release

FROM debian:bookworm-slim
RUN mkdir /app
RUN apt update && apt install -y curl
COPY --from=builder /app/target/release/cargo-cleaner /app/cargo-cleaner
COPY --from=builder /repos/cargo /root/cargo
COPY --from=builder /repos/alice /root/alice
COPY --from=builder /repos/bob /root/bob
COPY --from=builder /repos/carol /root/carol
COPY --from=builder /repos/dave /root/dave
COPY --from=builder /repos/eve /root/eve
COPY --from=builder /repos/kuma /root/kuma
RUN curl -sS https://starship.rs/install.sh > /tmp/install.sh
RUN sh /tmp/install.sh -y
RUN echo 'eval "$(starship init bash)"' > /root/.bashrc
RUN echo 'export PATH=$PATH:/app/' >> /root/.bashrc
WORKDIR /root
