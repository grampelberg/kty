# kuberift

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
