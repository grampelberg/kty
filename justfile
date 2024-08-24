# Colors

normal := '\033[0m'
red := '\033[31m'
green := '\033[32m'
yellow := '\033[33m'

# Version settings

git_version := `git rev-parse --short HEAD 2>/dev/null || echo "unknown"`
is_unstable := `git tag --points-at | grep 'v' && echo "" || echo "-UNSTABLE"`
version := `git cliff --bumped-version 2>/dev/null | cut -c2- || echo "0.0.0"` + is_unstable
version_placeholder := "0.0.0-UNSTABLE"

# Docker settings

registry := "ghcr.io/grampelberg"
image_name := "kuberift"
tag := "sha-" + git_version
image := registry + "/" + image_name + ":" + tag

tools:
    mise install

check: fmt-check lint audit

audit:
    cargo audit

fmt-check:
    cargo +nightly fmt --all --check
    just --fmt --unstable --check

lint:
    cargo clippy --no-deps

build-binary:
    cargo build --release --bin kuberift

build-image:
    docker build -t {{ image }} -f docker/kuberift.dockerfile .

login-ghcr:
    @if [ -z ${GHCR_USER+x} ] || [ -z ${GHCR_TOKEN+x} ]; then \
        echo "{{ red }}GHCR_USER and/or GHCR_TOKEN is not set.{{ normal }} See .envrc.example" && exit 1; \
    fi

    @echo "${GHCR_TOKEN}" | docker login ghcr.io -u "${GHCR_USER}" --password-stdin

upload-image:
    docker push {{ image }}

dev-push registry=env_var('LOCAL_REGISTRY'):
    just registry="{{ registry }}" tag="latest" build-image upload-image

extract-from-digests:
    #!/usr/bin/env bash
    set -euo pipefail

    mkdir -p /tmp/bins

    for digest in /tmp/digests/*/*; do
        sha="$(basename "${digest}")"
        bucket="$(basename $(dirname "${digest}"))"
        IFS=- read -r _ os arch <<< "${bucket}"
        name="kuberift-${os}-${arch}"
        echo "Extracting {{ image }}@sha256:${sha}"

        container_id="$(docker create --platform=${os}/${arch} {{ image }}@sha256:${sha})"
        docker cp "${container_id}:/usr/local/bin/kuberift" "/tmp/bins/${name}"
        docker rm "${container_id}"
    done

set-version:
    git grep -l "{{ version_placeholder }}" | grep -v "justfile" | xargs -I {} sed -i'.tmp' -e 's/{{ version_placeholder }}/{{ version }}/g' {}

helm-build:
    helm package helm --dependency-update --destination /tmp/chart

helm-upload token="GITHUB_TOKEN":
    echo "{{ "${" }}{{ token }}}" | helm registry login {{ registry }} -u gha --password-stdin
    helm push /tmp/chart/*.tgz oci://{{ registry }}/helm
