# Development

## Commits

We try to use [conventional commits][conventional commits]. This allows
[git-cliff][git-cliff] to construct a chagelog on release.

[conventional commits]: https://www.conventionalcommits.org/en/v1.0.0/
[git-cliff]: https://git-cliff.org

## CI

On PR, CI produces a darwin-arm64 binary and helm chart. Click on any step from
the PR and then `Summary` on the left sidebar to see the uploaded artifacts.
Docker images are not currently build/uploaded on PR runs.

## Environment

Copy `.envrc.example` to `.envrc`. The `GHCR_TOKEN` is a [personal access
token][pat] with permissions to `write:packages`. This is only required if you
want to upload directly to github.

[pat]:
  https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens

## Cluster

We recommend using [k3d][k3d] to run a local cluster. To setup:

```bash
k3d cluster create kuberift --registry-create kuberift:5432
```

Next, you'll want to add the registry to your `/etc/hosts`:

```bash
echo "127.0.0.1 kuberift" | sudo tee -a /etc/hosts
```

When you run `just dev-push`, an image at `kuberift:5432/kuberift:latest` will
be available to run inside the cluster.

[k3d]: https://k3d.io/v5.6.3/#releases

## Forwarding

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
