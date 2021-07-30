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
        libwebsockets-dev \
        m4 \
        make \
        pkg-config \
        wget

RUN apt-get -y --no-install-recommends install \
        gdb \
        gdbserver \
        gtk-doc-tools \
        libasan5

RUN aws configure set default.s3.max_bandwidth 50MB/s

## -----------------------------------------------------------------------------
## Install libnice 0.1.13 (signaling doesn't work in dev with newer versions)
## -----------------------------------------------------------------------------
RUN git clone https://gitlab.freedesktop.org/libnice/libnice \
    && cd libnice \
    && git checkout 0.1.13 \
    && ./autogen.sh \
    && ./configure \
    && make -j $(nproc) \
    && make install

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
## Install Paho MQTT client
## -----------------------------------------------------------------------------
ARG PAHO_MQTT_VERSION=1.3.5

RUN PAHO_MQTT_BUILD_DIR=$(mktemp -d) \
    && cd "${PAHO_MQTT_BUILD_DIR}" \
    && git clone "https://github.com/eclipse/paho.mqtt.c.git" . \
    && git checkout "v${PAHO_MQTT_VERSION}" \
    && make \
    && make install

## -----------------------------------------------------------------------------
## Install stable Rust
## -----------------------------------------------------------------------------
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

## -----------------------------------------------------------------------------
## Build Janus Gateway
## -----------------------------------------------------------------------------
ARG JANUS_GATEWAY_COMMIT='1da5aa226fb932c87fe70b35551057bd30c3713d'

RUN set -xe \
    && JANUS_GATEWAY_BUILD_DIR=$(mktemp -d) \
    && CFLAGS="-g -fsanitize=thread -fsanitize=address -fno-omit-frame-pointer" \
    && LDFLAGS="-lasan" \
    && cd "${JANUS_GATEWAY_BUILD_DIR}" \
    && git clone 'https://github.com/meetecho/janus-gateway' . \
    && git checkout "${JANUS_GATEWAY_COMMIT}" \
    && ./autogen.sh \
    && ./configure --prefix=/opt/janus --enable-post-processing \
    && make -j $(nproc) \
    && make install \
    && make configs \
    && rm -rf "${JANUS_GATEWAY_BUILD_DIR}"

## -----------------------------------------------------------------------------
## Install janus-conference-logger
## -----------------------------------------------------------------------------
COPY --from=netologygroup/janus-conference-logger:v0.1.1 \
    /build/target/release/libjanus_conference_logger.so \
    /opt/janus/lib/janus/loggers/libjanus_conference_logger.so
