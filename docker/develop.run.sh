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
    && cp ./docker/config/* ${JANUS_DIR}/etc/janus \
    && ${JANUS_DIR}/bin/janus
EOF

set -ex

BUILD_CACHE_VOLUME=janus-conference-cargo

mkdir -p recordings
docker volume create ${BUILD_CACHE_VOLUME}
docker build -t ${DOCKER_CONTAINER_NAME} -f docker/develop.dockerfile .
# Setting up `DOCKER_RUN_COMMAND` to be able to run initial command again.
docker run ${DOCKER_RUN_OPTIONS} \
    -v $(pwd):${PROJECT_DIR} \
    -v $(pwd)/recordings:/recordings \
    -v ${BUILD_CACHE_VOLUME}:/usr/local/cargo \
    -p ${DOCKER_WS_PORT}:8188 \
    -p 1883:1883 \
    -p 7088:7088 \
    -p 30000-30020:30000-30020 \
    -e "DOCKER_RUN_COMMAND=${DOCKER_RUN_COMMAND}" \
    --env-file $(pwd)/docker/janus.plugin.conference.environment \
    ${DOCKER_CONTAINER_NAME} \
    /bin/bash -c "set -x && /vernemq/bin/vernemq start && cd ${PROJECT_DIR} && ${DOCKER_RUN_COMMAND} && set +x && ${DOCKER_CONTAINER_COMMAND}"
