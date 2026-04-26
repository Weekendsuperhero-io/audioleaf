# syntax=docker/dockerfile:1.7
#
# Multi-arch container image for audioleaf on Raspberry Pi (and any Linux host
# with ALSA loopback). Bundles nqptp + shairport-sync (built from source for
# AirPlay 2 support) + audioleaf API server + prebuilt React frontend.
#
# Build:
#   docker buildx build --platform linux/arm64,linux/amd64 -t audioleaf:test .
#
# Run on a Pi (snd-aloop kernel module must be loaded on the host):
#   podman compose -f containers/compose.yaml up -d

ARG DEBIAN_RELEASE=bookworm
ARG RUST_VERSION=1
ARG NODE_VERSION=20

# ---------- Stage 1: Rust binary ----------
FROM rust:${RUST_VERSION}-${DEBIAN_RELEASE} AS audioleaf-builder
RUN apt-get update && apt-get install -y --no-install-recommends \
        libasound2-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked --bin audioleaf

# ---------- Stage 2: React frontend (pnpm) ----------
FROM node:${NODE_VERSION}-${DEBIAN_RELEASE}-slim AS web-builder
RUN npm install -g pnpm@10.33.0
WORKDIR /build/web
COPY web/package.json web/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile
COPY web/ ./
RUN pnpm run build

# ---------- Stage 3: nqptp + shairport-sync from source (AirPlay 2) ----------
FROM debian:${DEBIAN_RELEASE}-slim AS airplay-builder
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential git autoconf automake libtool ca-certificates \
        libpopt-dev libconfig-dev libasound2-dev \
        libavahi-client-dev libssl-dev libsoxr-dev \
        libplist-dev libsodium-dev uuid-dev libgcrypt-dev xxd libplist-utils \
        libavutil-dev libavcodec-dev libavformat-dev libswresample-dev \
    && rm -rf /var/lib/apt/lists/*

# Pin to upstream main at build time. Bump these when you want a refresh.
ARG NQPTP_REF=main
ARG SHAIRPORT_REF=master

WORKDIR /src
RUN git clone --depth 1 --branch ${NQPTP_REF} https://github.com/mikebrady/nqptp.git
RUN cd nqptp && autoreconf -fi && ./configure && make -j"$(nproc)" \
    && make install DESTDIR=/out

RUN git clone --depth 1 --branch ${SHAIRPORT_REF} https://github.com/mikebrady/shairport-sync.git
RUN cd shairport-sync && autoreconf -fi \
    && ./configure --sysconfdir=/etc \
        --with-alsa --with-soxr --with-avahi \
        --with-ssl=openssl --with-airplay-2 --with-metadata \
    && make -j"$(nproc)" \
    && make install DESTDIR=/out

# ---------- Stage 4: runtime ----------
FROM debian:${DEBIAN_RELEASE}-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
        tini ca-certificates \
        libasound2 alsa-utils \
        avahi-daemon dbus \
        libavcodec59 libavformat59 libavutil57 libswresample4 \
        libsoxr0 libplist3 libsodium23 libgcrypt20 libssl3 \
        libpopt0 libconfig9 libavahi-client3 \
    && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /var/run/dbus /root/.config/audioleaf /usr/local/share/audioleaf

COPY --from=airplay-builder /out/usr/local/ /usr/local/
COPY --from=audioleaf-builder /build/target/release/audioleaf /usr/local/bin/audioleaf
COPY --from=web-builder /build/web/dist/ /usr/local/share/audioleaf/web/

COPY piWebServer/shairport-sync.conf /etc/shairport-sync.conf
COPY piWebServer/asound-default-loopback.conf /etc/asound.conf
COPY containers/entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

ENV AUDIOLEAF_FRONTEND_DIR=/usr/local/share/audioleaf/web \
    AUDIOLEAF_SHAIRPORT_METADATA_PIPE=/tmp/shairport-sync-metadata

EXPOSE 8787

ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/entrypoint.sh"]
