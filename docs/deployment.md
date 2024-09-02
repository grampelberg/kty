# Deployment

The `kuberift` server needs access to your cluster's API server and credentials
to connect to it. There are a couple ways to do this:

- On cluster - you can run it on cluster. Check out the [helm chart][helm-chart]
  for an easy way to get started. By running it on cluster, you get access and
  credentials automatically. The server then needs to be exposed so that you can
  connect to it. This can be done by any TCP load balancer. If you're running in
  the cloud, setting the server's service to `type: LoadBalancer` is the
  easiest. Alternatives include using the [gateway api][gateway-api] or
  configuring your ingress controller to route TCP.
- Off cluster - if you're already using jump hosts to get into your cluster,
  kuberift can run there. All you need to do is create a `kubeconfig` that uses
  the correct service account. There are [some plugins][sa-plugin] to make this
  easy. You'll still need a valid `ClusterRole` and `ClusterRoleBinding` setup.
  Take a look at the sample [rbac][helm-rbac] to see what do to there.

[gateway-api]: https://gateway-api.sigs.k8s.io
[helm-chart]: #helm
[sa-plugin]:
  https://github.com/superbrothers/kubectl-view-serviceaccount-kubeconfig-plugin
[helm-rbac]: helm/templates/rbac.yaml

## Helm

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

## Server RBAC

The kuberift server needs to be able to:

- Impersonate users and groups.
- Manage `keys`.
- Optionally update the CRDs.

To do the minimum of this, you can use the following `ClusterRole` + `Role`. For
a more in-depth example, take a look at the
[helm config](helm/templates/rbac.yaml).

```yaml
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
name: impersonator
rules:
  - apiGroups:
      - ''
    resources:
      - users
      - groups
    verbs:
      - 'impersonate'
    # Restrict the groups/users that can be impersonated through kuberift.
    # resourceNames:
    #   - foo@bar.com
    #   - my_group
---
apiVersion: rbac.authorization.k8s.io/v1
kind: Role
metadata:
name: kuberift
rules:
  - apiGroups:
      - 'key.kuberift.com'
    resources:
      - keys
    verbs:
      - '*'
---
```
