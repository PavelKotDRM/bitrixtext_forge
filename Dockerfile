# syntax=docker/dockerfile:1

FROM rust:1.96-bookworm AS builder

RUN apt-get update \
    && apt-get install --yes --no-install-recommends \
        build-essential \
        libasound2-dev \
        libegl1-mesa-dev \
        libgtk-3-dev \
        libgl1-mesa-dev \
        libwayland-dev \
        libx11-xcb-dev \
        libxkbcommon-dev \
        libxi-dev \
        libxrandr-dev \
        pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /workspace

COPY Cargo.toml Cargo.lock build.rs ./
COPY src ./src

RUN cargo build --release --locked

FROM scratch AS artifact

ARG TARGETARCH

COPY --from=builder /workspace/target/release/bitrixtext_forge /bitrixtext_forge-linux-${TARGETARCH}