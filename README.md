# kuberift

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

To be authorized, either the user from the identity or one of the groups that
user is a member of need to have role bindings added to the cluster. The
`kuberift users grant` command is one way to go about this, but it is purposely
naive. To do something more flexible, you can check out `kubectl`:

```bash
kubectl create clusterrolebinding foo-bar-com --clusterrole=<my-role> --user=foo@bar.com
```

Note that you can use `RoleBinding` instead, but that comes with caveats. See
the design decisions section for an explanation of what's happening there.

## Server RBAC

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

## Minimum Permissions

- List pods at the cluster level.

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
