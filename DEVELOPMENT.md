# Development

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
