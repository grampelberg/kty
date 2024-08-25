# Chart

## OpenID

By default, the kuberift auth0 account is used for authentication. It provides
Github and Google as authentication backends. If you would like to use your own
OpenID provider, it can be configured under `server.openid`.

## External Access

As SSH is not a HTTP based protocol, we need to be able to forward TCP to the
kuberift service. This means that most of the ingress solutions that use HTTP to
multiplex over a single IP address/cloud load balancer don't work.

- Set `server.loadbalacing = true` and, if your provider supports it, you'll get
  an external IP address to use for SSH.
- If you'd like to try out the [GatewayAPI][gateway-api], the [Envoy
  Gateway][envoy-gateway] project is included as a dependency. Set
  `envoy.enabled = true` and add `gateway` to tags. That'll setup a
  `GatewayClass`, `Gateway` and `TCPRoute` set to allow access to kuberift. Note
  that this also exposes an IP address publicly via a `LoadBalancer` service -
  it is just the Envoy one this time instead of directly going to kuberift.

[gateway-api]: https://gateway-api.sigs.k8s.io
[envoy-gateway]: https://gateway.envoyproxy.io

## Autoscaling

Setting `server.autoscale = true` will add a naive HPA to the cluster. It will
scale up to `server.maxReplicas`.
