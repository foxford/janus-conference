FROM rust:latest as build-janus

## -----------------------------------------------------------------------------
## Installing dependencies
## -----------------------------------------------------------------------------
RUN set -xe \
    && apt-get update \
    && apt-get -y --no-install-recommends install \
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
    && USRSCTP_BUILD_DIR=$(mktemp -d) \
        && cd "${USRSCTP_BUILD_DIR}" \
        && git clone https://github.com/sctplab/usrsctp . \
        && ./bootstrap \
        && ./configure --prefix=/usr \
        && make -j $(nproc) \
        && make install \
        && rm -fr "${USRSCTP_BUILD_DIR}"

## -----------------------------------------------------------------------------
## Installing Janus Gateway
## -----------------------------------------------------------------------------
ARG JANUS_GATEWAY_VERSION=0.4.5

RUN set -xe \
    && JANUS_GATEWAY_BUILD_DIR=$(mktemp -d) \
    && cd "${JANUS_GATEWAY_BUILD_DIR}" \
    && git clone 'https://github.com/meetecho/janus-gateway' . \
    && git checkout "v${JANUS_GATEWAY_VERSION}" \
    && ./autogen.sh \
    && ./configure --prefix=/opt/janus \
    && make -j $(nproc) \
    && make install \
    && make configs

## -----------------------------------------------------------------------------
## Development image
## -----------------------------------------------------------------------------
FROM build-janus as dev
RUN set -xe \
    && apt-get install -y gdb \
    && rustup component add clippy-preview

## -----------------------------------------------------------------------------
## Build Janus Conference plugin for deploy
## -----------------------------------------------------------------------------
FROM rust:latest as build-plugin-deploy
WORKDIR /build
COPY . .
RUN set -xe \
    && apt-get update \
    && apt-get -y --no-install-recommends install \
        ca-certificates \
        libjansson-dev \
    && cargo build --release

## -----------------------------------------------------------------------------
## Build image ready to deploy
## -----------------------------------------------------------------------------
FROM debian:latest as deploy
ARG PLUGIN=libjanus_conference.so
WORKDIR /opt/janus
COPY --from=build-janus /opt/janus .
COPY --from=build-plugin-deploy /build/target/release/${PLUGIN} ./lib/janus/plugins/${PLUGIN}
