# TODO

- Panic handling needs some help. The server doesn't shut down, which is good -
  but it also doesn't disconnect, or tell the user anything - which is bad.
  There's also zero info output on panic using the dev dashboard.

- Implement multi-cluster.

## Documentation

- Getting started needs help, in particular:
  - Granting your user should probably go before the install instructions.
  - Say something about the error when you don't have authorization.
- Make the getting started on a real cluster instructions more clear. In
  particular, it seems like it is a little difficult to see the install commands
  and realize that's what you need to use.

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

- Is it possible to get the kubelet logs?

- Use SSH forwarding to get into the nodes.

  - Does it make sense to do the `nsenter` trick for some use cases? This
    requires privileged mode to work.

- There's some kind of lag happening when scrolling aggressively (aka, holding
  down a cursor). It goes fine for ~10 items and then has a hitch in the
  rendering.

- Terminal resizing isn't wired up for the dev dashboard.

- The way that layers work would be better served by something with ndarray. In
  particular, calculating what the area of a widget would be is ugly.

- Add an error screen if the ready channel is closed with an error. See
  widget/pod.rs.

- Add routing back in.

- Calculate visibility via areas + zindex to understand what needs to be
  rendered instead of just assuming the view will set all or only the top layer.

- It feels like it'd be nice to just try to run the default command and if it
  errors give the user the option to change. That way, going to the shell tab
  will ~immediately jump into the pod.

- Dashboard as a struct doesn't really make sense anymore, it should likely be
  converted over to a simple function.

- The initial coalesce in `Apex` is a little weird because of the initial
  loading screen - feels like it is jumping a couple frames.

- Move YAML over to viewport. Should viewport be doing syntax highlighting by
  default? How do we do a viewport over a set of lines that require history to
  do highlighting?

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

- Animate the egress/ingress tunnel lines. In particular, it would be nice to
  watch `Active` fade to `Listening` after a request goes through.

## SFTP

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

- Figure out why `git cliff` goes `0.2.0` -> `0.2.1` -> `0.3.0` instead of
  `0.2.2`.

- Add integration test for `kty resources install`.

- Add integration test for `helm install`.

- Move client_id and config_url to a build-time concern. I'm not sure this will
  be great for the development experience. Is there a way to have defaults but
  override them? Maybe with a dev instance of auth0?

## Deployment

- Add `kustomize` to allow for an easier getting started.
- Make `helm install` work if someone's checked the repo out.

## Misc

- Move to [bon](https://docs.rs/bon/latest/bon/) instead of `derive_builder`.
