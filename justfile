normal := '\033[0m'
red := '\033[31m'
green := '\033[32m'
yellow := '\033[33m'
image := "ghcr.io/grampelberg/kuberift"
git_version := `git rev-parse --short HEAD || echo "unknown"`
tag := image + ":sha-" + git_version

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
    docker build -t {{ tag }} -f docker/kuberift.dockerfile .

upload-image:
    @if [ -z ${GHCR_USER+x} ] || [ -z ${GHCR_TOKEN+x} ]; then \
        echo "{{ red }}GHCR_USER and/or GHCR_TOKEN is not set.{{ normal }} See .envrc.example" && exit 1; \
    fi

    @echo "${GHCR_TOKEN}" | docker login ghcr.io -u "${GHCR_USER}" --password-stdin
    docker push {{ tag }}
