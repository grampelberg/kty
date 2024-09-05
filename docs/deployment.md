# Deployment

The `kuberift` server needs access to your cluster's API server and credentials
to connect to it. There are a couple ways to do this:

- [On cluster](#on-cluster)
- [Off cluster](#off-cluster)

[gateway-api]: https://gateway-api.sigs.k8s.io
[helm-chart]: #helm
[sa-plugin]:
  https://github.com/superbrothers/kubectl-view-serviceaccount-kubeconfig-plugin
[helm-rbac]: helm/templates/rbac.yaml

## Features

All the functionality is controlled via feature flags in the server:

- `pty` - Dashboard when `ssh` happens.
- `sftp` - Enables `scp` and `sftp`.
- `ingress-tunnel` - Provides `ssh -L` forwarding from a local port to the
  cluster.
- `egress-tunnel` - Provides `ssh -R` forwarding from the cluster to a local
  port.

## Bring Your Own Provider

By default, kuberift provides Github and Google authentication via.
[auth0][auth0]. To get your own setup using auth0, check out their
[instructions][auth0-setup].

You can, alternatively, use your own provider. It must support the [device
code][device-code] flow and have a URL that has the openid configuration. Take a
look at the configuration for `kuberift serve` for the required values.

[auth0]: https://auth0.com
[auth0-setup]:
  https://auth0.com/docs/get-started/authentication-and-authorization-flow/device-authorization-flow/call-your-api-using-the-device-authorization-flow#prerequisites
[device-code]: https://www.oauth.com/oauth2-servers/device-flow/

## On-Cluster

Check out the [helm chart][helm-chart] for an easy way to get started. If not
using helm, there are some things to be aware of:

- Credentials need to be mounted into the pod, see [Server RBAC](#server-rbac)
  for a minimal list of permissions.
- You need the pod to be reachable from where you're running `ssh`. This can be
  done by any TCP load balancer. If you're running in the cloud, setting the
  server's service to `type: LoadBalancer` is the easiest. Alternatives include
  using the [gateway api][gateway-api] or configuring your ingress controller to
  route TCP.

### Helm

There is a provided `getting-started.yaml` set of values. To install this on
your cluster, you can run:

```bash
helm install kuberift oci://ghcr.io/grampelberg/helm/kuberift \
  -n kuberift --create-namespace \
  --version $(curl -L https://api.github.com/repos/grampelberg/kuberift/tags | jq -r '.[0].name' | cut -c2-) \
  -f https://raw.githubusercontent.com/grampelberg/kuberift/main/helm/getting-started.yaml
```

Note: this exposes the kuberift service externally by default. To get that IP
address, you can run:

```bash
kubectl -n kuberift get service server --output=jsonpath='{.status.loadBalancer.ingress[0].ip}'
```

For more detailed instructions, take a look at the [README][helm-readme].

[helm-readme]: helm/README.md

## Off-Cluster

If you're already using jump hosts to get into your cluster, kuberift can run
there. Here are some things to be aware of:

- Provide credentials by creating a `kubeconfig` that uses the correct service
  account. here are [some plugins][sa-plugin] to make this easy. You'll still
  need a valid `ClusterRole` and `ClusterRoleBinding` setup. Take a look at the
  sample [rbac][helm-rbac] to see what do to there.
- For `ingress-tunnel` support, you'll need to have the server running on a
  network that can reach IP addresses in the cluster (nodes, pods) and can
  resolve cluster DNS.
- For `egress-tunnel` support, you'll need to have the server itself reachable
  from any pod in the cluster. In addition, make sure to configure `--pod-name`,
  `--pod-uid` and `--pod-ip` to some real values in the `serve` command.

## Server RBAC

The kuberift server needs to be able to:

- Impersonate users and groups.
- Manage `keys`.
- Optionally update the CRDs.

To do the minimum of this, you can use the following `ClusterRole`. For a more
in-depth example, take a look at the [helm config](helm/templates/rbac.yaml).

```yaml
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
name: impersonator
rules:
  - apiGroups: ['']
    resources:
      - users
      - groups
    verbs:
      - impersonate
    # Restrict the groups/users that can be impersonated through kuberift.
    # resourceNames:
    #   - foo@bar.com
    #   - my_group
```
