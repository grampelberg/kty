normal := '\033[0m'
red := '\033[31m'
green := '\033[32m'
yellow := '\033[33m'
image := "ghcr.io/grampelberg/kuberift"
git_version := `git rev-parse --short HEAD 2>/dev/null || echo "unknown"`
image_tag := image + ":sha-" + git_version
version := `git cliff --bumped-version --tag-pattern "v.*" 2>/dev/null | cut -c2- || echo "0.0.0"`
version_placeholder := "0.0.0-UNSTABLE"

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
    docker build -t {{ image_tag }} -f docker/kuberift.dockerfile .

upload-image:
    @if [ -z ${GHCR_USER+x} ] || [ -z ${GHCR_TOKEN+x} ]; then \
        echo "{{ red }}GHCR_USER and/or GHCR_TOKEN is not set.{{ normal }} See .envrc.example" && exit 1; \
    fi

    @echo "${GHCR_TOKEN}" | docker login ghcr.io -u "${GHCR_USER}" --password-stdin
    docker push {{ image_tag }}

extract-from-digests:
    #!/usr/bin/env bash
    set -euo pipefail

    mkdir -p /tmp/bins

    for digest in /tmp/digests/*/*; do
        sha="$(basename "${digest}")"
        name="$(basename $(dirname "${digest}"))"
        echo "Extracting ${name}@sha256:${sha}"

        container_id="$(docker create {{ image }}@sha256:${sha})"
        docker cp "${container_id}:/usr/local/bin/kuberift" "/tmp/bins/${name}"
        docker rm "${container_id}"
    done

replace-version:
    rg -g '!justfile' "{{ version_placeholder }}" -l | xargs -I {} sed -i '' -e 's/{{ version_placeholder }}/{{ version }}/g' {}
