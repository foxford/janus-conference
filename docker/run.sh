#!/bin/bash

PROJECT="janus-conference"
PROJECT_DIR="/build"
PLUGIN="libjanus_conference.so"
DOCKER_CONTAINER_NAME="sandbox/${PROJECT}"
DOCKER_CONTAINER_COMMAND=${DOCKER_CONTAINER_COMMAND:-'/bin/bash'}
DOCKER_RUN_OPTIONS=${DOCKER_RUN_OPTIONS:-'-ti --rm'}
DOCKER_WSS_PORT=${DOCKER_WSS_PORT:-'8989'}
DOCKER_WS_PORT=${DOCKER_WS_PORT:-'8188'}
WS_CONFIG="/opt/janus/etc/janus/janus.transport.websockets.cfg"

read -r DOCKER_RUN_COMMAND <<-EOF
    perl -pi -e 's/(wss = )no/\${1}yes/' "${WS_CONFIG}" \
    && perl -pi -e 's/;(wss_port = 8989)/\${1}/' "${WS_CONFIG}" \
    && cargo build --release \
    && ln -s "${PROJECT_DIR}/target/release/${PLUGIN}" "/opt/janus/lib/janus/plugins/${PLUGIN}" \
    && /opt/janus/bin/janus --token-auth --token-auth-secret=xepohgue3Wa3yohg0aiP --event-handlers --debug-level=6
EOF

set -ex

BUILD_CACHE_VOLUME=janus-conference-cargo

docker volume create $BUILD_CACHE_VOLUME
docker build -t ${DOCKER_CONTAINER_NAME} -f docker/Dockerfile .
# Setting up `DOCKER_RUN_COMMAND` to be able to run initial command again.
docker run ${DOCKER_RUN_OPTIONS} \
    -v $(pwd):${PROJECT_DIR} \
    -v ${BUILD_CACHE_VOLUME}:/root/.cargo \
    -p ${DOCKER_WSS_PORT}:8989 \
    -p ${DOCKER_WS_PORT}:8188 \
    -e "DOCKER_RUN_COMMAND=${DOCKER_RUN_COMMAND}" \
    ${DOCKER_CONTAINER_NAME} \
    /bin/bash -l -c "set -x && cd ${PROJECT_DIR} && ${DOCKER_RUN_COMMAND} && set +x && ${DOCKER_CONTAINER_COMMAND}"