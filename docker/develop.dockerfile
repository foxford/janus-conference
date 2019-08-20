FROM alpine:latest

## -----------------------------------------------------------------------------
## Install dependencies
## -----------------------------------------------------------------------------
RUN apk add --update --no-cache \
      # Build & debug tools
      build-base \
      gcc \
      git \
      autoconf \
      automake \
      libtool \
      curl-dev \
      gdb \
      # Janus Gateway dependencies
      libressl-dev \
      libsrtp-dev \
      libconfig-dev \
      libmicrohttpd-dev \
      jansson-dev \
      opus-dev \
      libogg-dev \
      libwebsockets-dev \
      gengetopt \
      libnice-dev \
      # Janus Conference plugin dependencies
      gstreamer-dev \
      gstreamer-tools \
      gst-plugins-base-dev \
      gst-plugins-good \
      gst-plugins-bad \
      gst-plugins-ugly \
      gst-libav \
      libnice-gstreamer \
      ffmpeg \
      # Rust
      # TODO: install latest Rust from rustup when rustup 1.19 gets released
      rust \
      cargo

## -----------------------------------------------------------------------------
## Build Paho MQTT client
## -----------------------------------------------------------------------------
ARG PAHO_MQTT_VERSION=1.3.0

RUN PAHO_MQTT_BUILD_DIR=$(mktemp -d) \
    && cd "${PAHO_MQTT_BUILD_DIR}" \
    && git clone "https://github.com/eclipse/paho.mqtt.c.git" . \
    && git checkout "v${PAHO_MQTT_VERSION}" \
    && make \
    && make install \
    && rm -rf "${PAHO_MQTT_BUILD_DIR}"

## -----------------------------------------------------------------------------
## Build Janus Gateway
## -----------------------------------------------------------------------------
ARG JANUS_GATEWAY_COMMIT='642641f1e86be4a056076603b571cab7fb6b8a26'

RUN set -xe \
    && JANUS_GATEWAY_BUILD_DIR=$(mktemp -d) \
    && cd "${JANUS_GATEWAY_BUILD_DIR}" \
    && git clone 'https://github.com/netology-group/janus-gateway' . \
    && git checkout "${JANUS_GATEWAY_COMMIT}" \
    && ./autogen.sh \
    && ./configure --prefix=/opt/janus \
    && make -j $(nproc) \
    && make install \
    && make configs \
    && rm -rf "${JANUS_GATEWAY_BUILD_DIR}"
