# We need ffmpeg 4 to use OPUS in MP4 but Debian stretch has only ffmpeg 4.
FROM netologygroup/ffmpeg-docker:n4.1.3 as ffmpeg

# Using Debian here because official Rust image is based on Debian too.
FROM debian:stretch as build-janus

## -----------------------------------------------------------------------------
## Install dependencies
## -----------------------------------------------------------------------------
ARG PAHO_MQTT_VERSION=1.3.0

RUN set -xe \
    && apt-get update \
    && apt-get -y --no-install-recommends install \
        libconfig-dev \
        libmicrohttpd-dev \
        libjansson-dev \
        libnice-dev \
        libcurl4-openssl-dev \
        libsofia-sip-ua-dev \
        libopus-dev \
        libogg-dev \
        libwebsockets-dev \
        libsrtp2-dev \
        gengetopt \
        ca-certificates \
        git \
        libtool \
        m4 \
        automake \
        make \
        libgstreamer1.0-dev \
        libgstreamer-plugins-base1.0-dev \
        gstreamer1.0-plugins-base \
        gstreamer1.0-plugins-good \
        gstreamer1.0-plugins-bad \
        gstreamer1.0-plugins-ugly \
        gstreamer1.0-libav \
        libgstrtspserver-1.0-dev \
        wget \
        gdb \
        gdbserver \
        libasan3 \
    && PAHO_MQTT_BUILD_DIR=$(mktemp -d) \
        && cd "${PAHO_MQTT_BUILD_DIR}" \
        && git clone "https://github.com/eclipse/paho.mqtt.c.git" . \
        && git checkout "v${PAHO_MQTT_VERSION}" \
        && make \
        && make install

## -----------------------------------------------------------------------------
## Install nightly Rust
## -----------------------------------------------------------------------------
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH

RUN set -eux; \
    \
    url="https://static.rust-lang.org/rustup/dist/x86_64-unknown-linux-gnu/rustup-init"; \
    wget "$url"; \
    chmod +x rustup-init; \
    ./rustup-init -y --no-modify-path --default-toolchain nightly; \
    rm rustup-init; \
    chmod -R a+w $RUSTUP_HOME $CARGO_HOME; \
    rustup --version; \
    cargo --version; \
    rustc --version;

## -----------------------------------------------------------------------------
## Install FFmpeg
## -----------------------------------------------------------------------------
COPY --from=ffmpeg /build/bin/ffmpeg /usr/local/bin/ffmpeg

## -----------------------------------------------------------------------------
## Build Janus Gateway
## -----------------------------------------------------------------------------
ARG JANUS_GATEWAY_COMMIT='51987d066188bc88338aa69720d1f0f176cd4424'

RUN set -xe \
    && JANUS_GATEWAY_BUILD_DIR=$(mktemp -d) \
    && CFLAGS="-g -fsanitize=thread -fsanitize=address -fno-omit-frame-pointer" \
    && LDFLAGS="-lasan" \
    && cd "${JANUS_GATEWAY_BUILD_DIR}" \
    && git clone 'https://github.com/netology-group/janus-gateway' . \
    && git checkout "${JANUS_GATEWAY_COMMIT}" \
    && ./autogen.sh \
    && ./configure --prefix=/opt/janus \
    && make -j $(nproc) \
    && make install \
    && make configs \
    && rm -rf "${JANUS_GATEWAY_BUILD_DIR}"
