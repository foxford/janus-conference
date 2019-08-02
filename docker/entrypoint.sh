#!/bin/sh

PROJECT="janus-conference"
PROJECT_DIR="/build"
JANUS_DIR="/opt/janus"
PLUGIN="libjanus_conference.so"

set -x

cargo build \
  && ln -sfn ${PROJECT_DIR}/target/debug/${PLUGIN} ${JANUS_DIR}/lib/janus/plugins/${PLUGIN} \
  && cp ${PROJECT_DIR}/docker/config/* ${JANUS_DIR}/etc/janus \
  && ${JANUS_DIR}/bin/janus

set +x
/bin/sh
