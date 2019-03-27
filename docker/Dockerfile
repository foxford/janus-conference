# Using Debian here because official Rust image is based on Debian too.
FROM debian:stretch as build-janus

## -----------------------------------------------------------------------------
## Installing dependencies
## -----------------------------------------------------------------------------
ARG PAHO_MQTT_VERSION=1.1.0

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
    && PAHO_MQTT_BUILD_DIR=$(mktemp -d) \
        && cd "${PAHO_MQTT_BUILD_DIR}" \
        && git clone "https://github.com/eclipse/paho.mqtt.c.git" . \
        && git checkout "v${PAHO_MQTT_VERSION}" \
        && make \
        && make install

## -----------------------------------------------------------------------------
## Installing Janus Gateway
## -----------------------------------------------------------------------------
ARG JANUS_GATEWAY_COMMIT='2ac24be3826fbca9f17eeb92b6aac9da27bb4060'

RUN set -xe \
    && JANUS_GATEWAY_BUILD_DIR=$(mktemp -d) \
    && cd "${JANUS_GATEWAY_BUILD_DIR}" \
    && git clone 'https://github.com/meetecho/janus-gateway' . \
    && git checkout "${JANUS_GATEWAY_COMMIT}" \
    && ./autogen.sh \
    && ./configure --prefix=/opt/janus \
    && make -j $(nproc) \
    && make install \
    && make configs \
    && rm -rf "${JANUS_GATEWAY_BUILD_DIR}"

RUN set -xe \
    && apt-get remove -y \
        ca-certificates \
        git \
        libtool \
        m4 \
        automake \
        make

## -----------------------------------------------------------------------------
## Build Janus Conference plugin for deploy
## -----------------------------------------------------------------------------
FROM rust:latest as build-plugin-deploy
WORKDIR /build
COPY . .
RUN set -xe \
    && apt-get update \
    && apt-get -y --no-install-recommends install \
        libjansson-dev libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
        gstreamer1.0-plugins-base gstreamer1.0-plugins-good \
        gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly \
        gstreamer1.0-libav libgstrtspserver-1.0-dev \
    && cargo build --release

## -----------------------------------------------------------------------------
## Build image ready to deploy
## -----------------------------------------------------------------------------
FROM build-janus
ARG PLUGIN=libjanus_conference.so
WORKDIR /opt/janus
COPY --from=build-plugin-deploy /build/target/release/${PLUGIN} ./lib/janus/plugins/${PLUGIN}
