# Development

## Commits

- All commits must be signed to merge into `main`.
- We try to use [conventional commits][conventional commits]. This allows
  [git-cliff][git-cliff] to construct a chagelog on release.

[conventional commits]: https://www.conventionalcommits.org/en/v1.0.0/
[git-cliff]: https://git-cliff.org

## PRs

Because github doesn't have great support for merging PRs from the webui, PRs
must be merged from the command line:

```bash
git rebase -ff branch-name && git push origin branch-name
```

## CI

On PR, CI produces a darwin-arm64 binary and helm chart. Click on any step from
the PR and then `Summary` on the left sidebar to see the uploaded artifacts.
Docker images are built to verify that the `linux-amd64` binary can be built but
they are not pushed or uploaded anywhere.

## Environment

Copy `.envrc.example` to `.envrc`. The `GHCR_TOKEN` is a [personal access
token][pat] with permissions to `write:packages`. This is only required if you
want to upload directly to github.

[pat]:
  https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens

## Cluster

We recommend using [k3d][k3d] to run a local cluster. To setup:

```bash
k3d cluster create kty --registry-create kty:5432
```

Next, you'll want to add the registry to your `/etc/hosts`:

```bash
echo "127.0.0.1 kty" | sudo tee -a /etc/hosts
```

When you run `just dev-push`, an image at `kty:5432/kty:latest` will be
available to run inside the cluster.

[k3d]: https://k3d.io/v5.6.3/#releases

## Logging

The global debug level can be overly noisy. Instead of doing `-vvvv`, try:

```bash
RUST_LOG=none,kty=debug
```

If you'd like to see backtraces on panic, set `RUST_BACKTRACE`.

### Tracing Tree

It can be a little difficult to reason about how events filter through the
application. Towards that end, `dispatch` has `tracing::instrument` on it in
most places. This can be used to render a tree based on the spans that lets you
see what functions are being called and what their return values are. To see
this data, you can use the same format as `RUST_LOG` and export:

```bash
TRACING_TREE=none,kty=trace
```

## Ingress Tunnel

If testing port forwarding and running the service locally (aka not on the
cluster), you won't have access to any of the DNS or IP addresses that might be
forwarded. To work around this, modify `/etc/hosts`:

```txt
127.0.0.1 localhost.default.svc
```

Then you'll be able to test forwarding to localhost via:

```bash
ssh -L 9090:svc/default/localhost:9091 me@localhost -p 2222
```

Testing `pods` and `nodes` requires running on the cluster as the response from
`addr` for those is IP addresses.

## Egress Tunnel

If you're not running on the cluster, you'll want to:

- Make sure you're running from the same network (doable with some games with
  VPNs and local clusters).
- Set `HOSTNAME` to a pod in the cluster that is in your default namespace.
- Set `POD_UID` to the `metadata.uid` of the pod from `HOSTNAME`.
- Set `POD_IP` to the IP address of your host. You can get this by going into a
  pod and doing a `nslookup host.docker.internal`.

## Kubernetes Resources

When updating resources, make sure to update them in both places:

- [resources](/resources/) - Used for `kty resources`, primarily as part of
  getting started.
- [helm/templates](/helm/templates/) - Used for `helm install`
