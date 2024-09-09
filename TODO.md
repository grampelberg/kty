# TODO

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

## TUI

- Allow wrapping around with the tabs (so left from 0 goes to the end).

- Dashboard as a struct doesn't really make sense anymore, it should likely be
  converted over to a simple function.

- The initial coalesce in `Apex` is a little weird because of the initial
  loading screen - feels like it is jumping a couple frames.

- Is there a way to do FPS on a per-session basis with prometheus? Naively the
  way to do it would be to have a per-session label value, but that would be
  crazy for cardinality.

- There's a bug somewhere in `log_stream`. My k3d cluster restarted and while I
  could get all the logs, the stream wouldn't keep running - it'd terminate
  immediately. `stern` seemed to be working fine. Recreating the cluster caused
  everything to work again.

- Move over to something like
  [ratatui-textarea](https://github.com/rhysd/tui-textarea) for the inputs.

- Add an editor to allow for creation of resources (should it just be pods?).

- Rethink the pod detail view. The yaml doesn't feel like the most important
  thing to look at, neither do logs. Shell feels the closest, but that's not
  great either. Maybe something like `kubectl describe`? It could be multi-panel
  too. A log view + overview feels like it might be the most useful. Note that
  logs are particularly expensive to show right now as the default fetches
  _everything_ into memory (but doesn't try to render the entire thing every
  100ms).

## SFTP

- Allow globs in file paths, eg `/*/nginx**/etc/passwd`.
- Return an error that is nicer than "no files found" when a container doesn't
  have cat/ls.
- Test `rsync` SSH integration.

## SSH Functionality

- Allow `ssh` directly into a pod without starting the dashboard.

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

- Move client_id and config_url to a build-time concern.

## Misc

- Move to [bon](https://docs.rs/bon/latest/bon/) instead of `derive_builder`.
