#!/usr/bin/env bash

set -ex

if [[ "${LOCAL}" ]]; then

    if [[ ! ${GITHUB_TOKEN} ]]; then echo "GITHUB_TOKEN isn't specified" 1>&2; exit 1; fi

    ## Initializing deploy for a local machine

    NAMESPACE='testing'
    DOCKER_IMAGE_TAG="$(git rev-parse --short HEAD)"
else

    if [[ ! ${GITHUB_TOKEN} ]]; then echo "GITHUB_TOKEN isn't specified" 1>&2; exit 1; fi
    if [[ ! ${DOCKER_PASSWORD} ]]; then echo "DOCKER_PASSWORD isn't specified" 1>&2; exit 1; fi
    if [[ ! ${DOCKER_USERNAME} ]]; then echo "DOCKER_USERNAME isn't specified" 1>&2; exit 1; fi
    if [[ ! ${KUBE_SERVER} ]]; then echo "KUBE_SERVER isn't specified" 1>&2; exit 1; fi
    if [[ ! ${KUBE_TOKEN} ]]; then echo "KUBE_TOKEN isn't specified" 1>&2; exit 1; fi

    DOCKER_IMAGE_TAG="$(git rev-parse --short HEAD)"
    #docker build -t netologygroup/janus-gateway:${DOCKER_IMAGE_TAG} -f docker/Dockerfile.deploy .
    #echo ${DOCKER_PASSWORD} \
    #    | docker login -u ${DOCKER_USERNAME} --password-stdin
    #docker push netologygroup/janus-gateway:${DOCKER_IMAGE_TAG}

    ## Clone environment repo for deploy
    git clone git@github.com:netology-group/environment.git
    cd environment/ansible
    ansible-playbook -i hosts -u vknyazev --become -l janus deploy.yaml -e docker_image=netologygroup/janus-gateway:${DOCKER_IMAGE_TAG} --diff
fi

#    ## Initializing deploy for Travis CI
#
#    if [[ "${PRODUCTION_TAG}" ]]; then
#        NAMESPACE='production'
#        DOCKER_IMAGE_TAG="${PRODUCTION_TAG}"
#    else
#        NAMESPACE='staging'
#        DOCKER_IMAGE_TAG="$(git rev-parse --short HEAD)"
#    fi
#
#    mkdir -p ${HOME}/.local/bin
#    export PATH=${HOME}/.local/bin:${PATH}
#
#    curl -fsSLo kubectl "https://storage.googleapis.com/kubernetes-release/release/$(curl -s https://storage.googleapis.com/kubernetes-release/release/stable.txt)/bin/linux/amd64/kubectl" \
#        && chmod +x kubectl \
#        && mv kubectl "${HOME}/.local/bin"
#    curl -fsSLo skaffold "https://storage.googleapis.com/skaffold/releases/v0.18.0/skaffold-linux-amd64" \
#        && chmod +x skaffold \
#        && mv skaffold "${HOME}/.local/bin"
#
#    echo ${DOCKER_PASSWORD} \
#        | docker login -u ${DOCKER_USERNAME} --password-stdin
#
#    kubectl config set-cluster media --embed-certs --server ${KUBE_SERVER} --certificate-authority deploy/ca.crt
#    kubectl config set-credentials travis --token ${KUBE_TOKEN}
#    kubectl config set-context media --cluster media --user travis --namespace=${NAMESPACE}
#    kubectl config use-context media
#
#fi
#
#function FILE_FROM_GITHUB() {
#    local URI="${1}"; if [[ ! "${URI}" ]]; then echo "${FUNCNAME[0]}:URI isn't specified" 1>&2; exit 1; fi
#    mkdir k8s || true
#    local FILEPATH="k8s/$(basename $URI)"
#    curl -fsSL \
#        -H "authorization: token ${GITHUB_TOKEN}" \
#        -H 'accept: application/vnd.github.v3.raw' \
#        -o $FILEPATH \
#        "${URI}"
#}
#
#FILE_FROM_GITHUB "https://api.github.com/repos/netology-group/environment/contents/cluster/k8s/apps/janus-gateway/ns/${NAMESPACE}/janus-gateway-service.yaml"
#FILE_FROM_GITHUB "https://api.github.com/repos/netology-group/environment/contents/cluster/k8s/apps/janus-gateway/ns/${NAMESPACE}/janus-gateway-config.yaml"
#FILE_FROM_GITHUB "https://api.github.com/repos/netology-group/environment/contents/cluster/k8s/apps/janus-gateway/ns/${NAMESPACE}/janus-gateway.yaml"
#FILE_FROM_GITHUB "https://api.github.com/repos/netology-group/environment/contents/cluster/k8s/apps/janus-gateway/ns/${NAMESPACE}/skaffold.yaml"
#
#IMAGE_TAG="${DOCKER_IMAGE_TAG}" skaffold run -f k8s/skaffold.yaml -n "${NAMESPACE}"
