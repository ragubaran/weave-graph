# Self-Hosted & Enterprise Deployment Guide

Weave Graph is engineered from the ground up for **zero-cloud, private enterprise environments**. The entire core intelligence engine executes in pure Rust with zero network calls, zero third-party telemetry, zero external database services, and zero LLM dependencies.

For teams running in private VPCs, on-premise data centers, or compliance-restricted air-gapped networks, Weave Graph provides the **Custom Mode Profile** (`--features custom`), enabling query-layer access controls, architectural policy checks, runtime trace import, and optional snapshot synchronization. It does not provide a built-in SSO/OIDC integration.

---

## 1. Custom Mode Profile (`--features custom`)

The `custom` feature bundle builds the available enterprise feature set into a
single native binary. It does not by itself certify authentication,
provenance, policy, semantic-quality, or resource-envelope requirements.

```toml
# weave-graph-cli/Cargo.toml
custom = [
    "team",            # Multi-repo federation and Markdown documentation
    "hub",             # Centralized snapshot sync client and daemon
    "hub-provenance",  # Cryptographic snapshot verification
    "provenance",      # Optional note/link provenance primitives
    "rbac",            # Query-layer role-based access control and SCIM server
    "otel",            # OpenTelemetry OTLP trace span ingestion and latency overlays
    "policy-lint",     # Declarative architectural boundary linting and drift analytics
    "fts",             # BM25 full-text code search with AST synonym expansion
    "vector"           # AST-bounded semantic vector search with sqlite-vec
]
```

### Building the Enterprise Binary
```bash
# Clone the repository
git clone https://github.com/ragubaran/weave-graph.git
cd weave-graph

# Build optimized production binary with custom features
cargo build --release -p weave-graph-cli --features custom

# Install to system path
cp target/release/weave /usr/local/bin/
```

---

## 2. Role-Based Access Control (RBAC)

In an enterprise codebase, sensitive business logic (payment gateways, cryptographic keys, regulatory reporting, proprietary algorithms) should not be universally exposed to all developers, third-party contractors, or AI agent prompts.

### Query-Layer Enforcement Invariant
Weave Graph enforces RBAC **strictly at the storage and graph-traversal boundary** (Core Invariant 7). Masking is never applied as an export-only filter:
- CLI queries (`weave query`)
- Architectural reports (`weave report`)
- Neighborhood exports (`weave export`)
- MCP AI agent tools (`weave serve --mcp`)

All inherit the exact same security guard, but the *shape* of masking differs by consumer: `weave query`/`weave export` filter a masked symbol out entirely — querying it returns an unresolvable-symbol error, as if it didn't exist. `weave serve --mcp`'s tools instead render a masked node as a `<rbac: hidden>` placeholder (id preserved, symbol/path/kind/signature replaced, line numbers zeroed) — so an agent sees that *something* is there without seeing what. Both are the same underlying guard; the placeholder-vs-omit choice is about what each consumer's output shape can represent, not a security difference.

### Role Hierarchy & Symbol Visibility
There is no per-role permission grant system — only one role name is special-cased anywhere in the code:
- **`"internal"`**: bypasses masking entirely; sees the full graph, exported and internal alike.
- **Every other role name** (`"engineer"`, `"admin"`, `"contractor"`, `"auditor"`, or anything you make up) gets **identical** masked behavior: only exported/public symbols (`pub fn`, exported class/interface definitions — the same per-language visibility rule `weave check-contracts` hashes by) are visible; everything else renders as masked.

There is no path-based restriction (a `src/billing/`/`src/crypto/` prefix carries no special meaning) and no role hierarchy — role names beyond `"internal"` are free-form labels for your own audit trail/SCIM bookkeeping, not a permission grant this code reads differently.

### Configuration (`.weave/config.toml`)
```toml
[rbac.users]
alice = ["internal"]
bob = ["engineer"]
charlie = ["contractor"]
ci_pipeline = ["auditor"]
```

Masking itself engages purely based on whether `--as <subject>` is passed on the command line; an identity-less call always resolves to the built-in anonymous identity — there's no `enabled`/`anonymous_role` toggle to flip. The one `[rbac]`-table key that does exist governs a different question — not "does masking apply," but "is an identity-less session even allowed to start" (see `--require-as` below).

### Requiring an Identity for Shared Sessions (`--require-as`)
`weave serve --mcp`'s default — an omitted `--as` runs unmasked, same as any other RBAC-gated command — is correct for a human running `weave` against their own checkout, but is a real footgun for a shared or multi-tenant deployment (a CI runner, a proxied MCP endpoint serving multiple identities) where the operator forgot to pass `--as`. Two ways to close that gap, either is sufficient:
```bash
weave serve --mcp --require-as --as <subject>
```
```toml
# .weave/config.toml — refuses to start weave serve --mcp without --as, repo-wide
[rbac]
require_identity = true
```
Neither changes `weave query`/`weave report`/`weave export`'s own masking default — this only gates whether the MCP server process is willing to start at all without a bound identity.

### Auditing & Testing Identities with `--as <identity>`
Administrators and CI pipelines can simulate access views using the global `--as` flag:
```bash
# View callers as charlie — bob and charlie see the identical masked view (neither holds "internal")
weave --as charlie query "callers(PaymentGateway.charge)"

# Start an MCP agent session restricted to a specific identity
weave --as charlie serve --mcp
```

---

## 3. SCIM 2.0 Identity Provisioning

Weave Graph accepts role assignments pushed from an enterprise IdP through a generic SCIM 2.0 endpoint. An optional `github-auth` build can also resolve a supplied GitHub token through GitHub's authenticated-user, organization, and team APIs; configured membership mappings become RBAC roles. It is not generic OAuth/OIDC or interactive SSO.

```text
┌─────────────────────────────────────────────────────────────┐
│ Enterprise IdP (any SCIM 2.0 client: Okta / Azure AD /       │
│ Google Workspace / a custom provisioning script)             │
└──────────────────────────────┬──────────────────────────────┘
                               │ SCIM push (POST/DELETE, optional bearer token)
                               ▼
┌─────────────────────────────────────────────────────────────┐
│ Weave SCIM Server: weave rbac serve-scim (loopback only)     │
│ • POST / (provision), DELETE /{subject} (deprovision),       │
│   POST /sync (refresh), GET /  and GET /{subject} (read)     │
│ • Atomically writes to .weave/rbac-directory.toml            │
└──────────────────────────────┬──────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────┐
│ Graph Query Engine (CLI & MCP Server)                       │
│ • Evaluates RBAC Guard against resolved directory           │
│ • Redacts unauthorized AST nodes and edges at query layer   │
└─────────────────────────────────────────────────────────────┘
```

The server binds `127.0.0.1` only, on the same "trusted network, not exposed" model the base MCP server uses. Bearer-token authentication is optional, off by default: set `[rbac.scim] token = "<secret>"` in `.weave/config.toml` and every request must carry a matching `Authorization: Bearer <secret>` header, or the server rejects it with `401`; omit the key and the server accepts any local caller, exactly as before. There is no `/Groups` endpoint and no GitHub-specific mapping code anywhere; whatever role strings the IdP pushes for a subject land verbatim in `.weave/rbac-directory.toml`, and only `"internal"` carries any special meaning once there (see §2).

### 1. Running the SCIM Directory Server
Launch the loopback SCIM 2.0 daemon:
```bash
weave rbac serve-scim --port 9292
```

Requiring a token (recommended for any shared host — SCIM provisioning can elevate a subject to `"internal"`):
```toml
# .weave/config.toml
[rbac.scim]
token = "a-long-random-secret"
```
```bash
curl -X POST http://127.0.0.1:9292/ \
  -H "Authorization: Bearer a-long-random-secret" \
  -d '{"userName": "alice", "roles": ["internal"]}'
```

### 2. Provisioning Role Assignments
An IdP (or a custom script standing in for one) pushes a subject and its role list; the server writes it to `.weave/rbac-directory.toml`, e.g.:
```toml
[users]
alice = ["internal"]
charlie = ["contractor"]
```

### 3. Deprovisioning
When someone should lose access, the IdP sends `DELETE /{subject}` to the SCIM endpoint — `.weave/rbac-directory.toml` is updated immediately. But the *served* snapshot every `--as` resolution reads is deliberately decoupled from the file: it only refreshes on the next `POST /sync`. So deprovisioning is "not immediately, and not never" — a query made between the `DELETE` and the next `/sync` still resolves the old roles; the very next `/sync` (an explicit, operator-triggered refresh, never automatic) drops them for good (`impl.md` M3.4).

---

## 4. Architectural Policy Linting (`policy-lint`)

Prevent architectural erosion, unauthorized cross-layer calls, and dependency cycles in CI before code merges.

### Declaring Boundary Rules (`.weave/policy.yaml`)
Define architectural invariants in your repository root:

```yaml
rules:
  # Disallow direct database access from HTTP and UI controllers
  - disallow:
      from: "src/controllers"
      to: "src/db"

  # Enforce that payment processors only route through the gateway interface
  - require:
      from: "src/billing"
      to: "src/gateway/interface.rs"

  # Prevent third-party external integrations from calling core internal models
  - disallow:
      from: "src/integrations"
      to: "src/core/internal"
```

### Running the CI Policy Gate
```bash
# Evaluate policy rules against the live code graph
weave policy lint
```
If a developer introduces a call violating any boundary rule, `weave policy lint` names the rule and up to 5 example crossings (symbol + file, no line number), and always exits with a non-zero exit code — there is no config toggle to make it advisory:

```text
Policy: 3 rule(s) from .weave/policy.yaml
✗ [disallow] src/controllers -> src/db
    UserController.delete (src/controllers/user_controller.rs) -> DatabasePool.execute_raw (src/db/connection.rs)
```

### Detecting Architectural Drift (`weave policy drift`)
```bash
weave policy drift
```
Scans the graph for exactly two things — always advisory, never a CI-failing gate the way `weave policy lint` is:
1. **Dependency Cycles**: strongly-connected-component detection to identify recursive architectural loops.
2. **Orphaned Files**: files with zero *inbound* cross-file dependency (nothing calls into them from elsewhere).

---

## 5. Distributed Traces & Telemetry Overlay (`otel`)

Static analysis shows *what can be called*; runtime telemetry shows *what is actually executed, how often, and how slow it is*. Weave Graph merges runtime telemetry with static code intelligence.

### Ingesting OTLP Traces
Export traces from Jaeger, Datadog, or your OpenTelemetry Collector as standard OTLP/JSON files and import them:

```bash
weave traces import /path/to/telemetry-export.json
```
- Operates 100% offline via file ingest (no open listening port, maintaining Core Invariant 1).
- Automatically correlates span attributes (`code.function`, `service.name`, span name) with static AST graph symbols.

### Overlaying Performance Metrics
Once imported, `weave query` exposes runtime performance metrics alongside call hierarchies:

```bash
weave query "latency(PaymentService.process_transaction)"
```

**Output:**
```text
PaymentService.process_transaction: 14250 span(s), 20 error(s)
  p50 12400µs · p95 48200µs · p99 184600µs · total 512340000µs (min 890µs, max 201400µs)
```
One compact line per symbol — durations in microseconds, no separate "hotspot callers" breakdown (that's `weave blast`'s job, not `latency()`'s).

---

## 6. Centralized Snapshot Registry & Hub (`hub`)

In enterprise monorepos or multi-repo microservice fleets, parsing millions of lines of code on every developer laptop and CI runner wastes CPU cycles and battery. The **Weave Hub** centralizes graph indexing into a shared artifact service.

```text
┌─────────────────────────────────────────────────────────────┐
│ GitHub Actions / GitLab CI Trunk Runner                     │
│ 1. Git Push / Merge to main                                 │
│ 2. weave index                                              │
│ 3. weave sync push ──► [Weave Registry Server]              │
└─────────────────────────────────────────────────────────────┘
                               ▲
                               │ HTTP Pull
                               │
┌──────────────────────────────┴──────────────────────────────┐
│ Developer Laptop / PR CI Runner                             │
│ 1. weave sync pull --fallback-latest                        │
│ 2. weave index --incremental (pays only the diff since the  │
│    hydrated snapshot, not a full re-parse)                  │
└─────────────────────────────────────────────────────────────┘
```

### 6.0 Registry Authentication & Snapshot Provenance (`hub-provenance`)

Both are opt-in, off by default, and layer independently on top of the loopback-only binding above:

| Flag | Config equivalent (client) | Default | Purpose |
| :--- | :--- | :--- | :--- |
| `weave-registry --auth-token <token>` | `.weave/config.toml`'s `[hub] token` | none — unauthenticated | Every request must carry a matching `Authorization: Bearer <token>`, or the registry rejects it. |
| `weave-registry --provenance-key <secret-u64>` | `weave sync push --signature <hex>` | none — unverified | Every push's `X-Weave-Signature` (hex-encoded bytes) must verify against this secret before the registry commits it; a missing, wrong, or tampered signature is rejected with `400` and never advances the repo's head. |

```bash
weave-registry --bind 0.0.0.0:8080 --data-dir /data \
  --max-queue-depth-per-repo 1000 --max-pushes-per-minute-per-repo 600 \
  --auth-token "$(openssl rand -hex 32)" \
  --provenance-key 8891273649102837465
```

```toml
# .weave/config.toml — client side
[hub]
url = "https://weave-registry.internal.corp"
token = "same bearer token the registry was started with"
```

`--provenance-key` binds `MockSnapshotProvenanceVerifier::with_key(<secret>)` — a shared secret both sides must know, **never** the verifier's default key (that key is a public constant in the OSS binary; using it would look like verification while accepting anything). A deployment computes its own signature client-side — for example, an external Lodestone Nexus provenance service — and attaches it via `weave sync push --signature <hex>`; the registry only ever checks what it's configured to check. Omitting `--provenance-key` keeps every push unverified. Because the bundled verifier uses a symmetric key, this proves *integrity and shared-secret possession*, not non-repudiation or PKI; a deployment requiring Merkle/PKI trust must supply that external provider and verification service.

### 6.1 Ready-to-Use Deployment Manifests (`deploy/`)

The repository includes pre-configured, hardened deployment manifests in the [`deploy/`](../../deploy) directory:

| Deployment Target | Manifest Location | Architecture & Security Properties |
| :--- | :--- | :--- |
| **Docker Compose** | [`deploy/docker/`](../../deploy/docker) | Distroless non-root container (`uid 65532`) + Nginx TLS 1.3 reverse proxy sidecar. |
| **Kubernetes / EKS / GKE** | [`deploy/kubernetes/`](../../deploy/kubernetes) | Single-replica Deployment + ClusterIP Service + TLS Ingress (`deployment.yaml`, `ingress.yaml`). |
| **Linux Bare-Metal / VM** | [`deploy/systemd/`](../../deploy/systemd) | Hardened `systemd` service unit (`ProtectSystem=strict`, private state at `/var/lib/weave-registry`). |

> [!NOTE]
> **Design Invariant**: `weave-registry` is designed to run plaintext over localhost or a private container network namespace. All public traffic **must terminate TLS one hop in front** via reverse proxy (Nginx, Envoy, ALB, or Ingress Controller).

---

### 6.2 Docker & Docker Compose Deployment

Build the minimal distroless container image and launch with an Nginx TLS termination sidecar:

```bash
# 1. Build the registry container image from workspace root
docker build -f deploy/docker/Dockerfile -t weave-registry:latest .

# 2. Launch the registry + TLS proxy composition
cd deploy/docker
docker compose up -d
```

**Custom Response Header Forwarding**:
Any reverse proxy (Nginx, Envoy, Traefik) terminating TLS in front of `weave-registry` must forward these custom response headers:
- `X-Weave-Signature`: Carries the hex-encoded signature sidecar on `pull`, and (on `push`) is what the registry checks against `--provenance-key` when configured (see §6.0).
- `Upload-Offset`: Enables resumable chunked snapshot uploads on `HEAD`.
- `Retry-After`: Communicates backoff times when rate-limited.

---

### 6.3 AWS Production Deployment Guide

#### Option A: AWS ECS (Fargate) + EFS + Application Load Balancer (Recommended)
Deploy containerized `weave-registry` with serverless execution and persistent network storage:

1. **Persistent Volume (EFS)**:
   - Create an Amazon EFS filesystem with an Access Point configured with POSIX UID/GID `65532:65532` and path `/weave-registry`.
2. **Task Definition (`weave-registry-task.json`)**:
   ```json
   {
     "family": "weave-registry",
     "networkMode": "awsvpc",
     "requiresCompatibilities": ["FARGATE"],
     "cpu": "1024",
     "memory": "2048",
     "containerDefinitions": [
       {
         "name": "weave-registry",
         "image": "<aws_account_id>.dkr.ecr.<region>.amazonaws.com/weave-registry:latest",
         "essential": true,
         "portMappings": [{"containerPort": 8080, "protocol": "tcp"}],
         "command": [
           "--bind", "0.0.0.0:8080",
           "--data-dir", "/data",
           "--max-queue-depth-per-repo", "1000",
           "--max-pushes-per-minute-per-repo", "600"
         ],
         "mountPoints": [
           {
             "sourceVolume": "registry-efs",
             "containerPath": "/data",
             "readOnly": false
           }
         ],
         "linuxParameters": {"initProcessEnabled": true}
       }
     ],
     "volumes": [
       {
         "name": "registry-efs",
         "efsVolumeConfiguration": {
           "fileSystemId": "fs-xxxxxxxx",
           "transitEncryption": "ENABLED",
           "authorizationConfig": {
             "accessPointId": "fsap-xxxxxxxx"
           }
         }
       }
     ]
   }
   ```
3. **Application Load Balancer (ALB)**:
   - HTTPS Listener on port `443` using ACM SSL Certificate.
   - Target Group: Port `8080` (HTTP), Target Type `IP`, Health Check Path `/healthz` (or root `/`).

#### Option B: AWS EKS (Kubernetes)
Deploy using the native Kubernetes manifests:
```bash
# Apply PVC, Deployment, and Service
kubectl apply -f deploy/kubernetes/deployment.yaml

# Apply AWS Load Balancer Controller Ingress
kubectl apply -f deploy/kubernetes/ingress.yaml
```

---

### 6.4 GCP Production Deployment Guide

#### Option A: GCP Cloud Run + Cloud Filestore / Cloud Storage FUSE (Recommended)
Deploy an autoscaling containerized registry with managed TLS on Google Cloud:

1. **Storage Setup (Filestore NFS / GCS Bucket)**:
   - Provision a Cloud Filestore Basic SSD instance or standard GCS bucket for snapshot artifacts.
2. **Deploy to Cloud Run via gcloud CLI**:
   ```bash
   # Deploy container with mounted storage volume and internal VPC ingress
   gcloud run deploy weave-registry \
     --image gcr.io/<gcp_project_id>/weave-registry:latest \
     --platform managed \
     --region us-central1 \
     --port 8080 \
     --cpu 2 \
     --memory 2Gi \
     --ingress internal-and-cloud-load-balancing \
     --execution-environment gen2 \
     --add-volume name=registry-storage,type=cloud-storage,bucket=weave-registry-snapshots \
     --add-volume-mount volume=registry-storage,mount-path=/data \
     --args="--bind,0.0.0.0:8080,--data-dir,/data,--max-queue-depth-per-repo,1000,--max-pushes-per-minute-per-repo,600"
   ```
3. **Cloud Load Balancing & Google-Managed Certificate**:
   - Point an External HTTPS Load Balancer with Cloud Armor and Google-managed SSL certificate to the Cloud Run Serverless Network Endpoint Group (NEG).

#### Option B: Google Kubernetes Engine (GKE)
Deploy onto GKE with Google Compute Engine Persistent Disk (`standard-rwo` / `premium-rwo`):
```bash
kubectl apply -f deploy/kubernetes/deployment.yaml
kubectl apply -f deploy/kubernetes/ingress.yaml
```

---

### 6.5 Bare-Metal Linux VM Deployment (`systemd`)

For dedicated Linux virtual machines (EC2, GCP Compute Engine, or on-premise servers):

```bash
# 1. Copy binary to system path
sudo cp target/release/weave-registry /usr/local/bin/

# 2. Create service unit and dedicated data directory
sudo mkdir -p /var/lib/weave-registry
sudo cp deploy/systemd/weave-registry.service /etc/systemd/system/

# 3. Enable and start service
sudo systemctl daemon-reload
sudo systemctl enable --now weave-registry
```

---

### 6.6 Client Configuration & CI Acceleration Workflow

Configure client repositories to sync with your deployed registry in `.weave/config.toml`:

```toml
[hub]
url = "https://weave-registry.internal.corp"
snapshot_retention = 20
# Required only if the registry was started with --auth-token (§6.0).
token = "same bearer token the registry was started with"
```

In PR test pipelines, replace expensive full reindexing with instant snapshot hydration:
```bash
# Hydrates the graph snapshot for the merge-base commit (atomic file swap,
# no cold source-tree parse)
weave sync pull --fallback-latest

# Reindexes only the modified files in the pull request
weave index --incremental

# Run fast blast radius check
weave blast --base origin/main --format md --out pr-comment.md
```

---

## 7. Complete CI/CD Pipeline Reference

Below is a complete enterprise GitHub Actions workflow demonstrating the integration of Weave Graph Custom Mode features:

```yaml
name: Code Intelligence & Architecture Gates

on:
  pull_request:
    branches: [main]
  push:
    branches: [main]

jobs:
  weave-intelligence:
    runs-on: ubuntu-latest
    steps:
      - name: Checkout code
        uses: actions/checkout@v4
        with:
          fetch-depth: 0 # Full history required for merge-base blast radius

      - name: Download Weave Graph Custom Binary
        run: |
          curl -sSL https://internal-releases.corp/weave-custom-linux-x86_64 -o /usr/local/bin/weave
          chmod +x /usr/local/bin/weave

      - name: Hydrate Graph Snapshot from Hub
        run: |
          weave sync pull --fallback-latest || weave index

      - name: Verify Multi-Repo Contract Compatibility
        run: |
          weave check-contracts --diff --scoped

      - name: Evaluate Architectural Policy Boundaries
        run: |
          weave policy lint

      - name: Check for Architecture Drift and Dependency Cycles
        run: |
          weave policy drift

      - name: Generate PR Blast Radius Report
        if: github.event_name == 'pull_request'
        run: |
          weave blast --base origin/main --format md --out blast-report.md
          gh pr comment ${{ github.event.pull_request.number }} --body-file blast-report.md
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}

      - name: Publish Canonical Graph Snapshot
        if: github.event_name == 'push' && github.ref == 'refs/heads/main'
        run: |
          weave index
          weave sync push
```
