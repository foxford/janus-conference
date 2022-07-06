#!/usr/bin/env bash

if [[ ! ${GITHUB_TOKEN} ]]; then echo "GITHUB_TOKEN isn't specified" 1>&2; exit 1; fi

PROJECT="${PROJECT:-janus-conference}"
SOURCE=${SOURCE:-'https://api.github.com/repos/netology-group/ulms-env/contents/k8s'}
BRANCH="${BRANCH:-master}"

function FILE_FROM_GITHUB() {
    local DEST_DIR="${1}"; if [[ ! "${DEST_DIR}" ]]; then echo "${FUNCNAME[0]}:DEST_DIR isn't specified" 1>&2; exit 1; fi
    local URI="${2}"; if [[ ! "${URI}" ]]; then echo "${FUNCNAME[0]}:URI isn't specified" 1>&2; exit 1; fi

    mkdir -p "${DEST_DIR}"
    curl -fsSL \
        -H "authorization: token ${GITHUB_TOKEN}" \
        -H 'accept: application/vnd.github.v3.raw' \
        -o "${DEST_DIR}/$(basename $URI)" \
        "${URI}?ref=${BRANCH}"
}

function ADD_PROJECT() {
    local _PATH="${1}"; if [[ ! "${_PATH}" ]]; then echo "${FUNCNAME[0]}:_PATH is required" 1>&2; exit 1; fi
    local _PROJECT="${2}"; if [[ ! "${_PROJECT}" ]]; then echo "${FUNCNAME[0]}:PROJECT is required" 1>&2; exit 1; fi

    tee "${_PATH}" <<END
PROJECT=${_PROJECT}
$(cat "${_PATH}")
END
}

set -ex

## Use the same project for mdbook script.
FILE_FROM_GITHUB "deploy" "${SOURCE}/utils/ci-mdbook.sh"
ADD_PROJECT "deploy/ci-mdbook.sh" "${PROJECT}"
chmod u+x deploy/ci-mdbook.sh
