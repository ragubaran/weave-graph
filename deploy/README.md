> **Scope**: deployment manifests for `weave-registry`
> **Rule**: the registry itself is plaintext HTTP/1.1 by design (loopback/VPC); TLS is terminated one hop in front — by the reverse proxy (`nginx.conf`), by the sidecar, or by the cluster ingress (`kubernetes/ingress.yaml`). Nothing here opens a plaintext port beyond a private interface.

## Contents

| Path                                                               | What it is                                                                                                    |
| ------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------- |
| [`docker/Dockerfile`](docker/Dockerfile)                           | Multi-stage build → distroless, non-root (`uid 65532`), state only in the mounted data volume                 |
| [`docker/docker-compose.yml`](docker/docker-compose.yml)           | Single-node deployment: registry + nginx TLS-terminating proxy                                                |
| [`docker/nginx.conf`](docker/nginx.conf)                           | TLS termination proxy for the plaintext registry                                                              |
| [`systemd/weave-registry.service`](systemd/weave-registry.service) | Bare-metal unit: hardening flags, `ProtectSystem=strict`, private `/var/lib/weave-registry` state             |
| [`kubernetes/deployment.yaml`](kubernetes/deployment.yaml)         | Single-replica Deployment (spool state is local-disk; scale by sharding `repo_id`, not by replicas) + Service |
| [`kubernetes/ingress.yaml`](kubernetes/ingress.yaml)               | TLS termination at the ingress; pods stay plaintext inside the cluster                                        |

## Rate limits are operator-supplied on purpose

`RegistryConfig` has no `Default`: queue depth and pushes-per-minute must be
calibrated against the deploying team's observed merge rates — the values
in these manifests are starting points, not recommendations.

## A TLS-terminating proxy must forward two response headers

The registry now supports chunked/resumable upload and snapshot signatures.
Both round-trip through custom response headers a naive proxy can silently
drop: `X-Weave-Signature` (on `pull`, carries the `.sig`
sidecar) and `Upload-Offset` (on `HEAD`, resumes an interrupted push).
`docker/nginx.conf` forwards both explicitly; a hand-rolled proxy or an
ingress controller with its own header allowlist needs the same. Request
headers (`X-Weave-Base-Sha`, `X-Weave-Retention`, `X-Weave-Signature` on
push) need no such allowlisting — proxies forward request headers by
default; only response headers are ever selectively hidden.

**Not yet wired end to end**: `weave sync push/pull` (the CLI client) does
not call the signature trait yet. The transport above works today if you
sign and pass a signature yourself through `weave-graph-hub::HubClient`
directly; `weave sync` doesn't do that on your behalf yet.

## `--bind` placeholder

Every manifest previously shipped `--bind [IP_ADDRESS]:8080` as a literal,
unfillable placeholder — worse, the square brackets made `docker-compose.yml`
and `kubernetes/deployment.yaml` invalid YAML (`[IP_ADDRESS]` opens a flow
sequence; confirmed with a real `docker compose config` failure before the
fix). Replaced with real, working defaults instead of a placeholder:

- `docker-compose.yml` / `kubernetes/deployment.yaml` / `Dockerfile`'s `CMD`:
  `0.0.0.0:8080` — safe because the port is reachable only through what
  published it (compose `expose`, a k8s `ClusterIP` Service, `docker run -p`),
  never directly from outside that network boundary.
- `systemd/weave-registry.service`: `127.0.0.1:8080` — bare-metal has no
  container network boundary, so it binds loopback and expects a same-host
  reverse proxy in front, same shape as the compose deployment.
