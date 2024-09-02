# kuberift

SSH into your Kubernetes cluster! kuberift is an SSH server that provides a
TUI-based dashboard in your cluster. There's no need to manage SSH keys however.
It works with OpenID providers, such as Github or Google and impersonates users
to provide them with the same permissions they normally would have - all based
on the standard RBAC mechanisms.

You can:

- Get a shell in running pods - just like you would with SSH normally.
- Access the logs for running and exited containers in a pod.
- Forward a local port remotely, allowing access to services and pods in the
  cluster.
  - `scp` files from pods. sftp clients work as well.

![demo](./assets/demo.gif)

## Getting Started

1. Download the [cli][cli-download] and add it to your `$PATH`.
1. Get a k8s cluster. [k3d][k3d] is a convenient way to get a cluster up and
   running fast. Follow their installation instructions and create a default
   cluster.
1. Grant your email address access to the cluster. Choose `cluster-admin` if
   you'd like something simple to check out how things work. For more details on
   the minimum possible permissions, read the [Authorization][auth] section. The
   email address is what you'll be using to authenticate against. It can either
   be the one associated with a google or github account. Note, the ID used for
   login and the providers available can all be configured.

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

- `scp` some files out of a container:

  ```bash
  scp -P 2222 me@localhost:/default/my-pod/etc/hosts /tmp
  ```

[cli-download]: https://github.com/grampelberg/kuberift/releases
[k3d]: https://k3d.io

## Interaction

### SSH

To get to the dashboard, you can run:

```bash
ssh anything@my-remote-host-or-ip -p 2222
```

The provided username is not used as your identity is authenticated via other
mechanisms.

### Port Forward

You can forward requests from a local port into a resource on the remote
cluster. The supported resources are `nodes`, `pods` and `services`. See the
[authorization][auth] section for details on required RBAC.

To forward port 9090 on your local system to 80 on the cluster, you can run:

```bash
ssh me@my-cluster -p 2222 -L 9090:service/default/remote-service:80
```

The first time 9090 is accessed, a connection will be made. Pay attention to the
dashboard as any errors establishing this session will be reflected there.

The connection string format is `<resource>/<namespace>/<name>`. As nodes are
not namespaced, the format is `<resource>/<name>`.

Unlike the API server proxy, this works for any TCP service and is not limited
to HTTP/HTTPS. For example, you can ssh directly to a node in the cluster with:

```bash
ssh me@my-cluster -p 2222 -L 3333:no/my-node:22
```

With that running in one terminal, you can run this in another:

```bash
ssh my-node-username@localhost -p 3333
```

### SFTP

The cluster is represented by a file tree:

```bash
/<namespace>/<pod-name>/<container-name>/<file-path>
```

For the `nginx` pod running in `default`, you would do something like:

```bash
scp -P 2222 me@localhost:/default/nginx/nginx/etc/hosts /tmp
```

It can be a little easier to navigate all this with an sftp client as that'll
render the file tree natively for you.

## Releases

- See releases for the latest tagged release.
- The `unstable` tag is updated on every merge to main.

## Documentation

- [Architecture](docs/architecture.md)
- [Auth][auth] - Deep dive on what's happening around auth and what the minimum
  permissions are for each piece of functionality.
- [Deployment](docs/deployment.md) - Figure out how to get running on your own
  cluster.
- [Development](DEVELOPMENT.md) - Some tips and tricks for doing development on
  kuberift itself.
- [Metrics](docs/metrics.md) - List of the possible metrics exported via.
  prometheus.
- [TODO](TODO.md) - A selection of outstanding functionality.

[auth]: docs/auth.md
