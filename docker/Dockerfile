FROM rust:latest

## -----------------------------------------------------------------------------
## Installing dependencies
## -----------------------------------------------------------------------------
RUN set -xe \
    && apt-get update \
    && apt-get -y --no-install-recommends install \
        ca-certificates \
        libmicrohttpd-dev \
        libjansson-dev \
        libnice-dev \
        libcurl4-openssl-dev \
        libsofia-sip-ua-dev \
        libglib2.0-dev \
        libopus-dev \
        libogg-dev \
        libwebsockets-dev \
        libsrtp2-dev \
        gengetopt \
        libtool \
        automake \
        cmake \
        git \
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
RUN set -xe \
    && JANUS_GATEWAY_VERSION='0.4.5' \
    && JANUS_GATEWAY_BUILD_DIR=$(mktemp -d) \
    && cd "${JANUS_GATEWAY_BUILD_DIR}" \
    && git clone 'https://github.com/meetecho/janus-gateway' . \
    && git checkout "v${JANUS_GATEWAY_VERSION}" \
    && ./autogen.sh \
    && ./configure --prefix=/opt/janus \
    && make -j $(nproc) \
    && make install \
    && make configs
