# Using Debian here because official Rust image is based on Debian too.
FROM debian:buster as build-janus

## -----------------------------------------------------------------------------
## Install dependencies
## -----------------------------------------------------------------------------
RUN set -xe \
    && apt-get update \
    && apt-get -y --no-install-recommends install \
    autoconf \
    automake \
    awscli \
    ca-certificates \
    curl \
    ffmpeg \
    gengetopt \
    git \
    libavformat-dev \
    libavcodec-dev \
    libconfig-dev \
    libcurl4-openssl-dev \
    libglib2.0-dev \
    libjansson-dev \
    libmicrohttpd-dev \
    libogg-dev \
    libopus-dev \
    libsofia-sip-ua-dev \
    libssl-dev \
    libtool \
    m4 \
    make \
    pkg-config \
    wget

RUN apt-get -y --no-install-recommends install \
    ninja-build \
    python3 \
    python3-pip \
    python3-setuptools \
    python3-wheel

RUN aws configure set default.s3.max_bandwidth 10MB/s

## -----------------------------------------------------------------------------
## Install latest libnice (recommended by Janus devs)
## -----------------------------------------------------------------------------
RUN pip3 install meson \
    && git clone https://gitlab.freedesktop.org/libnice/libnice \
    && cd libnice \
    && meson --prefix=/usr build \
    && ninja -C build \
    && ninja -C build install

## -----------------------------------------------------------------------------
## Install libsrtp (with --enable-openssl option)
## -----------------------------------------------------------------------------
ARG LIBSRTP_VERSION=2.3.0

RUN wget https://github.com/cisco/libsrtp/archive/v${LIBSRTP_VERSION}.tar.gz \
    && tar xfv v${LIBSRTP_VERSION}.tar.gz \
    && cd libsrtp-${LIBSRTP_VERSION} \
    && ./configure --prefix=/usr --enable-openssl \
    && make shared_library \
    && make install

## -----------------------------------------------------------------------------
## Build Janus Gateway
## -----------------------------------------------------------------------------
ARG JANUS_GATEWAY_COMMIT='v0.11.4'

RUN set -xe \
    && JANUS_GATEWAY_BUILD_DIR=$(mktemp -d) \
    && cd "${JANUS_GATEWAY_BUILD_DIR}" \
    && git clone 'https://github.com/meetecho/janus-gateway' . \
    && git checkout "${JANUS_GATEWAY_COMMIT}" \
    && ./autogen.sh \
    && ./configure --prefix=/opt/janus --enable-post-processing  \
    && make -j $(nproc) \
    && make install \
    && make configs \
    && rm -rf "${JANUS_GATEWAY_BUILD_DIR}"

RUN set -xe \
    && apt-get remove -y \
    autoconf \
    automake \
    git \
    libtool \
    m4 \
    make \
    ninja-build \
    wget

## -----------------------------------------------------------------------------
## Build Janus Conference plugin for deploy
## -----------------------------------------------------------------------------
FROM buildpack-deps:buster as build-plugin-deploy

ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH

RUN set -eux; \
    url="https://static.rust-lang.org/rustup/dist/x86_64-unknown-linux-gnu/rustup-init"; \
    wget "$url"; \
    chmod +x rustup-init; \
    ./rustup-init -y --no-modify-path --default-toolchain stable; \
    rm rustup-init; \
    chmod -R a+w $RUSTUP_HOME $CARGO_HOME; \
    rustup --version; \
    cargo --version; \
    rustc --version;

RUN set -xe \
    && apt-get update \
    && apt-get -y --no-install-recommends install libjansson-dev 

WORKDIR /build

COPY Cargo.* ./
RUN mkdir ./src && touch src/lib.rs
RUN cargo build --release

COPY src/ ./src/
RUN touch src/lib.rs && cargo build --release

## -----------------------------------------------------------------------------
## Build image ready to deploy
## -----------------------------------------------------------------------------
FROM build-janus
ARG PLUGIN=libjanus_conference.so
WORKDIR /opt/janus
COPY --from=build-plugin-deploy /build/target/release/${PLUGIN} ./lib/janus/plugins/${PLUGIN}
COPY ./scripts/upload_record.sh /opt/janus/bin/upload_record.sh

COPY --from=foxford/janus-conference-logger:v0.1.1 \
    /build/target/release/libjanus_conference_logger.so \
    /opt/janus/lib/janus/loggers/libjanus_conference_logger.so

EXPOSE 8088
