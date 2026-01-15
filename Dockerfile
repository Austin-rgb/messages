FROM rust:1.71 AS builder

WORKDIR /usr/src/app

COPY Cargo.toml Cargo.lock ./
RUN cargo fetch

COPY . .

RUN cargo build --release

FROM debian:bullseye-slim


COPY --from=builder /usr/src/app/target/release/messages /usr/local/bin/messages

CMD [ "messages" ]
