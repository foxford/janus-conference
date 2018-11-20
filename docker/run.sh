#!/bin/bash

PROJECT="janus-conference"
PROJECT_DIR="/build"
JANUS_DIR="/opt/janus"
PLUGIN="libjanus_conference.so"
DOCKER_CONTAINER_NAME="sandbox/${PROJECT}"
DOCKER_CONTAINER_COMMAND=${DOCKER_CONTAINER_COMMAND:-'/bin/bash'}
DOCKER_RUN_OPTIONS=${DOCKER_RUN_OPTIONS:-'-ti --rm'}
DOCKER_WS_PORT=${DOCKER_WS_PORT:-'8188'}

read -r DOCKER_RUN_COMMAND <<-EOF
    cargo build \
    && (rm "${JANUS_DIR}/lib/janus/plugins/${PLUGIN}" || true) \
    && ln -s "${PROJECT_DIR}/target/debug/${PLUGIN}" "${JANUS_DIR}/lib/janus/plugins/${PLUGIN}" \
    && ${JANUS_DIR}/bin/janus --event-handlers --debug-level=5
EOF

set -ex

BUILD_CACHE_VOLUME=janus-conference-cargo

docker volume create $BUILD_CACHE_VOLUME
docker build -t ${DOCKER_CONTAINER_NAME} -f docker/Dockerfile.dev .
# Setting up `DOCKER_RUN_COMMAND` to be able to run initial command again.
docker run ${DOCKER_RUN_OPTIONS} \
    -v $(pwd):${PROJECT_DIR} \
    -v ${BUILD_CACHE_VOLUME}:/usr/local/cargo \
    -p ${DOCKER_WS_PORT}:8188 \
    -e "DOCKER_RUN_COMMAND=${DOCKER_RUN_COMMAND}" \
    ${DOCKER_CONTAINER_NAME} \
    /bin/bash -c "set -x && cd ${PROJECT_DIR} && ${DOCKER_RUN_COMMAND} && set +x && ${DOCKER_CONTAINER_COMMAND}"