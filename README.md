# kuberift

## Getting Started

1. Download the [cli][cli-download] and add it to your `$PATH`.
1. Get a k8s cluster. [k3d][k3d] is a convenient way to get a cluster up and
   running fast. Follow their [installation] instructions and create a default
   cluster.
1. Grant your email address access to the cluster. Choose `cluster-admin` if
   you'd like something simple to check out how things work. For more details on
   the minimum possible permissions, read the [Authorization] section. The email
   address is what you'll be using to authenticate against. It can either be the
   one associated with a google or github account. Note, the ID used for login
   and the providers available can all be configured.

   ```bash
   kuberift users grant <cluster-role> <email-address>
   ```

1. Start the server.

   ```bash
   kuberift --serve
   ```

1. SSH into your cluster!

   ```bash
   ssh -o UserKnownHostsFile=/dev/null -o StrictHostKeyChecking=no -p 2222 me@localhost
   ```

From this point, here's a few suggestions for things to check out:

- Start a new pod. It'll show up in the dashboard immediately!
- Exec into a pod. Select the pod you want and go to the `Shell` tab. You'll be
  able to pick the command to exec and then be shell'd into the pod directly.
- Follow the logs. Logs for all containers in a pod are streamed to the `Logs`
  tab when you've selected a pod from the main list.

[cli-download]: https://github.com/grampelberg/kuberift/releases
[k3d]: https://k3d.io

## Deployment

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

### Bring Your Own Provider

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

## Identity (Authentication)

Access is managed via k8s' RBAC system. This is managed with `User` and `Group`
subjects in role bindings. Kuberift impersonates a user with optional groups.
Authorization is then managed by k8s itself.

There are two ways for an incoming SSH session to get a user identity:

- OpenID - If the user does not have an authorized public key, the SSH session
  prompts with an open id flow. When that flow is successful, the returned token
  is mapped to a k8s identity. By default, this is the `email` claim in the
  identity token. If you would like to map different claims and/or add groups,
  take a look at the server configuration.
- Public Key - By default, once a user has been authenticated with open id, they
  will have a public key. This will contain the user and group information
  extracted from the identity token. If you would like to skip OpenID entirely,
  you can create `Key` resources, the `kuberift users key` can be used to do
  this as an alternative to `kubectl`.

To validate that a user has access, you can use the `kuberift users check`
command. This is a great way to debug why users are not being allowed to
connect.

```bash
kuberift users check foo@bar.com
```

## Authorization

To be authorized, either the name or groups for a user need to have role
bindings added to the cluster. The `kuberift users grant` command is one way to
go about this, but it is purposely naive. To do something more flexible, you can
check out `kubectl`:

```bash
kubectl create clusterrolebinding foo-bar-com --clusterrole=<my-role> --user=foo@bar.com
```

Note that you can use `RoleBinding` instead, but that comes with caveats. See
the design decisions section for an explanation of what's happening there.

### Minimum Permissions

- List pods at the cluster level.

## Metrics

- bytes_received_total - Total number of bytes received. This should be keyboard
  input.
- channel_bytes_sent_total - Total number of bytes sent via a channel by IO type
  (blocking, non-blocking). This is what the UI and raw modes use to send data
  to the client. It will be different that the amount of bytes `russh` itself.
- ssh_clients_total - Number of incoming connections.
- ssh_session_errors_total - Number of non-IO related unhandled errors at the
  session level.
- session_total - Number of sessions created.
- active_sessions - Number of currently active sessions.
- session_duration_minutes - Duration of a session in minutes.
- unexpected_state_total - Number of times an unexpected state was encountered.
  This should only be incremented if there's a bug.
- auth_attempts_total - Number of authentication attempts by method (publickey,
  interactive). This can seem inflated because `publickey` will always be
  attempted first and `interactive` will happen at least twice for every
  success.
- auth_results_total - Number of auth responses returned by method and result
  (accept, partial, reject). Note that this can seem inflated because
  `publickey` is always attempted first and provides a rejection before moving
  onto other methods.
- auth_succeeded_total - Number of fully authn and authz'd users. After this,
  users can request a PTY.
- code_generated_total - Number of codes generated for users. This is the first
  half of the `interactive` mode.
- code_checked_total - Number of codes that have been checked by result (valid,
  invalid). This is the second half of the `interactive` mode and it is possible
  that users retry after getting `invalid` because of something on the openid
  provider side.
- window_resize_total - Number of times the window has been asked to resize.
- container_exec_duration_minutes - Number of minutes a raw terminal was running
  exec'd into a pod.
- table_filter_total - Number of times a table was filtered.
- widget_views_total - Number of times a widget was created by resource
  (container, pod) and type (cmd, log, yaml, ...).

## Design Decisions

- Instead of having a separate `User` CRD to track the user, we rely on k8s'
  users/groups which are subjects on (Cluster)RoleBindings. Identity is
  extracted from the openid tokens via claims (email by default) and that is
  used to map to k8s concepts. The `Key` resource maps the values from a
  previous token to the SSH key used during the original authentication attempt.
  This key expires when the token itself would have and can be created manually
  with any desired expiration.

- The minimum viable access is `list` for `pods` across all namespaces.
  Understanding what subset of a cluster users can see is a PITA. This is
  because k8s cannot filter `list` requests to a subset. When combined with the
  fact that `SelfSubjectRulesReview` only works on a per-namespace basis, it
  becomes extremely expensive to understand what an individual user can see
  across the entire cluster. This will be updated in the future but requires
  namespaces to be available via UI elements.

## Releases

- See releases for the latest tagged release.
- The `unstable` tag is updated on every merge to main.

## TODO

- Groups are probably what most users are going to want to use to configure all
  this. The closest to the OpenID spec would be via adding extra scopes that add
  the data required to the token and then map back to a group. Imagine:

  ```yaml
  user: email
  group: https://myapp.example.com/group
  ```

  The downside to using this kind of configuration is that it'll need to be
  handled in the provider backend and it is unclear how easy that'll be. It is
  possible in auth0, so I'll go down this route for now.

- Is there a way to do FPS on a per-session basis with prometheus? Naively the
  way to do it would be to have a per-session label value, but that would be
  crazy for cardinality.
