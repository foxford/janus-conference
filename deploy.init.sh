#!/usr/bin/env bash

## Initializing deploy for Travis CI
if [[ "${TRAVIS}" ]]; then
    if [[ "${TRAVIS_TAG}" ]]; then
        NAMESPACE='production'
    else
        NAMESPACE='staging'
    fi
fi

if [[ ! ${NAMESPACE} ]]; then echo "NAMESPACE isn't specified" 1>&2; exit 1; fi
if [[ ! ${GITHUB_TOKEN} ]]; then echo "GITHUB_TOKEN isn't specified" 1>&2; exit 1; fi

SOURCE=${SOURCE:-'https://api.github.com/repos/netology-group/environment/contents/cluster/k8s'}

function FILE_FROM_GITHUB() {
    local DEST_DIR="${1}"; if [[ ! "${DEST_DIR}" ]]; then echo "${FUNCNAME[0]}:DEST_DIR isn't specified" 1>&2; exit 1; fi
    local URI="${2}"; if [[ ! "${URI}" ]]; then echo "${FUNCNAME[0]}:URI isn't specified" 1>&2; exit 1; fi

    mkdir -p "${DEST_DIR}"
    curl -fsSL \
        -H "authorization: token ${GITHUB_TOKEN}" \
        -H 'accept: application/vnd.github.v3.raw' \
        -o "${DEST_DIR}/$(basename $URI)" \
        "${URI}"
}

set -ex

FILE_FROM_GITHUB "deploy" "${SOURCE}/deploy/s3-docs.sh"

chmod u+x deploy/s3-docs.sh

