# TODO

- Panic handling needs some help. The server doesn't shut down, which is good -
  but it doesn't tell the user anything - which is bad. There's also zero info
  output on panic using the dev dashboard.

- Implement multi-cluster.

## Documentation

## Authorization

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

  Note: it looks like Google might require addition verification to get the
  `groups()` scope "externally".

## Server

- Move over to axum for health instead of warp.
- Terminate the session (but not the server) on panic.

## TUI

- There needs to be some way to pre-flight permissions for a component so that
  an error is shown instead of letting the component fail.

- Terminal resizing isn't wired up for the dev dashboard.

- The way that layers work would be better served by something with ndarray. In
  particular, calculating what the area of a widget would be is ugly.

- Add routing back in.

- Calculate visibility via areas + zindex to understand what needs to be
  rendered instead of just assuming the view will set all or only the top layer.

- It feels like it'd be nice to just try to run the default command and if it
  errors give the user the option to change. That way, going to the shell tab
  will ~immediately jump into the pod.

- Dashboard as a struct doesn't really make sense anymore, it should likely be
  converted over to a simple function.

- Move over to something like
  [ratatui-textarea](https://github.com/rhysd/tui-textarea) for the inputs.

- Add an editor to allow for creation of resources (should it just be pods?).

- Animate the egress/ingress tunnel lines. In particular, it would be nice to
  watch `Active` fade to `Listening` after a request goes through.

### Nodes

- Is it possible to get the kubelet logs?

  - `kubectl get --raw "/api/v1/nodes/node-1.example/proxy/logs/` works as
    `NodeQueryLog` which is beta in 1.30. It is a little weird though, k3s at
    least returns html? And it doesn't contain the kubelet logs?

- Use SSH forwarding to get into the nodes.

  - Does it make sense to do the `nsenter` trick for some use cases? This
    requires privileged mode to work.

  - This is waiting on the next release of russh as `handle.open_channel_agent`
    just landed.

### Pods

- Rethink the pod detail view. The yaml doesn't feel like the most important
  thing to look at, neither do logs. Shell feels the closest, but that's not
  great either. Maybe something like `kubectl describe`? It could be multi-panel
  too. A log view + overview feels like it might be the most useful. Note that
  logs are particularly expensive to show right now as the default fetches
  _everything_ into memory (but doesn't try to render the entire thing every
  100ms).

- Add `graph` to the pod view.

### Table

- Highlight filter matches in the list.

## SFTP

- Get a watchman style demo working.
- Allow globs in file paths, eg `/*/nginx**/etc/passwd`.
- Return an error that is nicer than "no files found" when a container doesn't
  have cat/ls.
- Test `rsync` SSH integration.

## SSH Functionality

- Allow `ssh` directly into a pod without starting the dashboard.
- How can `ssh` be shutdown without causing it to have a 1 as an exit code?

## Ingress Tunnel

## Egress Tunnel

- Cleanup services/endpoints on:
  - Shutdown - especially termination of the channel.
  - Startup - because we can't do cross-namespace owner references, anything
    created that doesn't have an active pod should be removed (via `targetRef`
    on the `endpointSlice`).
- Test what happens when a service is replaced. It looks like the endpointslice
  sticks around but it is unclear if the separate endpointslice's endpoints are
  used or not.

## Build

- Add integration test for `kty resources install`.

- Add integration test for `helm install`.

- Move client_id and config_url to a build-time concern. I'm not sure this will
  be great for the development experience. Is there a way to have defaults but
  override them? Maybe with a dev instance of auth0?

## Misc

- Move to [bon](https://docs.rs/bon/latest/bon/) instead of `derive_builder`.
