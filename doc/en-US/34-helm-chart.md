---
title: Helm chart — configuration
summary: How to install Taba on Kubernetes with the Helm chart — what it creates, every value (node, secrets, Ingress, backup to built-in (SeaweedFS) or external S3, Prometheus or Telegraf collector, OTLP, KEDA autoscaling), how each one becomes node configuration and what the chart refuses.
type: reference
status: stable
verified-on: 2026-10-17 @ 4dc8c9f
sources: [deploy/helm/taba/Chart.yaml, deploy/helm/taba/values.yaml, deploy/helm/taba/values.schema.json, deploy/helm/taba/templates, deploy/helm/tests, crates/taba-node/src/config/params/mod.rs, docker-compose.yml]
---

# Helm chart — configuration

> Read first: [22-configuration.md](22-configuration.md) (the node keys), [23-operations.md](23-operations.md) (scale, upgrade, drain) and [33-s3-backup.md](33-s3-backup.md). The chart's operating modes, each with its tests, are in [35-helm-use-cases.md](35-helm-use-cases.md).

The chart lives in `deploy/helm/taba/`. It translates what `docker-compose.yml` does (`init`, nodes, SeaweedFS, load balancer) to Kubernetes and adds what only makes sense there: Ingress, a metrics collector and autoscaling.

```bash
helm install taba deploy/helm/taba -n taba --create-namespace
kubectl -n taba get pods -w                      # nodes become "ready" after joining the cluster
kubectl -n taba get secret taba-secrets -o jsonpath='{.data.root-token}' | base64 -d
```

Every Taba version publishes the chart as an OCI artifact on GHCR, signed and attested, with `version` and `appVersion` equal to the release version ([27-ci-cd.md](27-ci-cd.md)): `helm install taba oci://ghcr.io/rafaelpontani/charts/taba --version 0.1.0 -n taba --create-namespace`. The candidates (`0.1.0-rc.N`) are published too, and Helm only picks them with an explicit `--version` or `--devel`.

Prerequisites: Kubernetes 1.27+, a default `StorageClass` (one volume per node) and access to the `rafaelpontani/taba` image. KEDA, the Prometheus Operator and an Ingress controller are **optional** and only matter when the matching feature is turned on.

## What the chart creates

| Resource | What for |
|---|---|
| `StatefulSet` `<release>` | The nodes. One volume per node at `/var/lib/taba`; `enableServiceLinks: false` (Services would inject `TABA_PORT`, `TABA_SERVICE_HOST`… and the node reads the `TABA_*` prefix as configuration) |
| `Service` `<release>` | The **client API** (8080). The only one an Ingress or a load balancer should reach |
| `Service` `<release>-entry` | **Cluster entry** (`cluster.entry-address`), port 8081, plain TCP. It only has **ready** pods: with no ready node nobody answers, which is how the first node knows it must form the cluster |
| `Service` `<release>-headless` | Pod identity (`taba-0.taba-headless`), includes pods still bootstrapping |
| `Service` `<release>-ops` | Operations port (9090) for the collector. **No authentication**: never leaves the cluster |
| `ConfigMap` `<release>` | The node's `taba.yml` (the `config` tree of the values) |
| `Secret` `<release>-secrets` | `root-token`, `tls.crt`, `tls.key`. Generated **once** by the pre-install Job (`taba init`); not deleted on `helm uninstall` nor recreated on upgrade |
| `PodDisruptionBudget` | At most 1 node unavailable |
| `Ingress` | Optional (`ingress.enabled`) |
| SeaweedFS (`StatefulSet`, `Service`, `Secret`, bucket Job) | Optional (`seaweedfs.enabled`) |
| Prometheus or Telegraf (`Deployment`, `ConfigMap`, RBAC) or `ServiceMonitor` | Optional (`collector.type`) |
| `ScaledObject` (KEDA) | Optional (`autoscaling.enabled`) |
| `NetworkPolicy` | Optional (`networkPolicy.enabled`) |

Decisions the chart makes for you (from [23-operations.md](23-operations.md)):

- **First node.** The pod with ordinal 0 starts with `cluster.first-node=true`. It is safe to leave on forever: if the others are up, it joins them instead of forming a new cluster.
- **Pod IP.** `cluster.advertise-addresses` receives the pod IP (downward API). Without it the node would find its own IP by resolving `entry-address`, which only answers once a ready node exists.
- **Probes** on the operations port: `startup` and `liveness` on `/health/live`, `readiness` on `/health/ready`.
- **Upgrade** one node at a time (`maxUnavailable: 1`) and `podManagementPolicy: Parallel` (new nodes keep trying to join on their own until the first one is ready).
- **Stop.** `terminationGracePeriodSeconds` must cover `drain.timeout` + `backup.drain.timeout` + 5 s; the chart **refuses** smaller values (the pod would be killed before handing its messages to another node).

## Values

### Nodes

| Value | Default | Effect |
|---|---|---|
| `image.repository` / `image.tag` | `rafaelpontani/taba` / chart `appVersion` (`0.1` in the repository; the exact version in the published chart) | All nodes must run the same version. The published chart points to the image of the **same** version; installing from the repository uses the safe pointer `0.1` ([27-ci-cd.md](27-ci-cd.md)) |
| `replicaCount` | `3` | Production: never fewer than `max(3, replication.factor)`. The chart refuses fewer, unless `allowFewReplicas=true` (tests only). Ignored with `autoscaling.enabled` |
| `terminationGracePeriodSeconds` | `120` | See above; minimum `drain.timeout + backup.drain.timeout + 5` |
| `ports.api` / `ports.cluster` / `ports.operations` | `8080` / `8081` / `9090` | Must differ (the node does not start with repeated ports). They become `api.port`, `cluster.port`, `operations.port` |
| `persistence.size` / `storageClass` / `accessModes` | `10Gi` / cluster default / `ReadWriteOnce` | Volume for `storage.dir` (`/var/lib/taba`, whose last folder is already named `taba`, as the node requires) |
| `persistence.retentionPolicy.whenScaled` | `Delete` | When replicas are reduced the node leaves drained and comes back as a **new node**: the old volume is useless |
| `resources`, `nodeSelector`, `tolerations`, `affinity`, `topologySpreadConstraints`, `priorityClassName` | empty | Kubernetes defaults. Without `affinity` the chart asks (preferred) for nodes on different hosts |
| `podSecurityContext` / `containerSecurityContext` | uid/gid/fsGroup `10001`, unprivileged | The image user is uid 10001 |
| `probes.*` | startup up to 10 min | Adjust if the initial sync is longer |
| `extraEnv`, `extraEnvFrom`, `extraVolumes`, `extraVolumeMounts` | empty | Escape hatch: any `TABA_*`, or `AWS_*` for the AWS credential chain |
| `serviceAccount.annotations` | empty | E.g. `eks.amazonaws.com/role-arn` for S3 backup without keys (IRSA) |
| `podDisruptionBudget.enabled` / `maxUnavailable` | `true` / `1` | |
| `networkPolicy.enabled` / `apiFrom` / `opsFrom` | `false` | On: API open (or only `apiFrom`); 8081 and 9090 only for the release pods and `opsFrom` |

### Node configuration (`config`)

The `config` tree is the node's `taba.yml`: **each dot in the key becomes a level** ([22-configuration.md](22-configuration.md)). Anything not set stays at the node default. `values.yaml` carries all the relevant keys commented, with the default beside them. Example:

```yaml
config:
  replication: { factor: 3 }
  message: { max-bytes: 1MB }
  retention: { ttl: { default: 7d } }
  ratelimit: { publish: { rps: 500 }, burst: 1000 }
  client-ip: { trusted-proxies: [10.0.0.0/8] }   # your Ingress controller's CIDR
  operations: { allowed-ips: [10.0.0.0/8] }
  telemetry: { topics: { mode: by-tags } }
```

The node **refuses an unknown key** (a typo prevents startup), so a wrong `config` shows up on the first `helm install --wait`. The chart controls and **overrides**: `api.port`, `operations.port`, `cluster.port`, `cluster.entry-address`, `storage.dir`, `secrets.dir` and, with backup, `backup.s3.bucket`/`endpoint`/`region`/`prefix`/`path-style`. The parameters the node accepts and ignores ([22-configuration.md](22-configuration.md)) are not in `values.yaml`.

`config.replication.factor: 1` (the one-node profile) starts **one node**, with no authentication, no cluster TLS, no Secret and no Job (for a lab). The old name, `config.mode: development`, still works and is the same profile; the chart does not pass it to the node.

Changing `config` changes a checksum on the pod and the nodes are restarted **one at a time** on `helm upgrade`.

### Chart validations

`helm install`/`upgrade`/`template` fails with a message in Portuguese when:

- `replicaCount` is below `max(3, replication.factor)` (without `allowFewReplicas`);
- `terminationGracePeriodSeconds` is below `drain.timeout` + `backup.drain.timeout` + 5 s;
- the three ports repeat;
- `config.write.quorum` is not between 1 and `replication.factor`;
- `config.backup.disk` does not satisfy `evict-to < evict-above < pause-at ≤ 100`;
- `seaweedfs.enabled` and `backup.s3.bucket` are both on;
- `backup.s3.accessKey` and `secretKey` do not come as a pair;
- `collector.type`, `config.mode` (`production` or `development`), `config.telemetry.logs.format` or `config.telemetry.topics.mode` have an invalid value (`values.schema.json`);
- autoscaling has `minReplicas` below the safe minimum, is in the one-node profile (`replication.factor: 1`), has no Prometheus, or uses `disk.source=app` without `capacityBytes`.

### Cluster secrets

| Value | Default | Effect |
|---|---|---|
| `secrets.existingSecret` | empty | Your own Secret, with the keys `root-token`, `tls.crt` and `tls.key`. Empty = the pre-install Job runs `taba init` **once** |
| `secrets.init.certDays` | `1825` | Validity of the generated certificate |
| `secrets.init.kubectlImage` | `alpine/k8s` | Only the generation Job uses it (`kubectl create secret`) |

The Job only **creates** the Secret if it does not exist, and the Secret has no Helm owner: `helm uninstall` keeps it (delete it by hand to start over). `root-token` accepts **two lines** (new and previous) to rotate without a restart ([14-security.md](14-security.md)); the kubelet refreshes the mounted file within ~1 minute and the node re-reads it every `auth.reload-interval`.

### Service and Ingress

```yaml
ingress:
  enabled: true
  className: nginx            # empty = default class; some controllers want the legacy annotation
  annotations:
    nginx.ingress.kubernetes.io/proxy-body-size: 6m   # message.max-bytes (5MB) + envelope
  hosts:
    - host: taba.example.com
      paths: [{ path: /, pathType: Prefix }]
  tls:
    - secretName: taba-tls
      hosts: [taba.example.com]
```

- The Ingress points **only** to the `api` port of the Service. Ports 8081 (mTLS between nodes) and 9090 (operations, no authentication) **do not exist** on the API Service and must not be exposed by any path.
- Terminate TLS at the Ingress: the node API is HTTP ([14-security.md](14-security.md)).
- Raise the controller's body limit above `message.max-bytes` (5 MB), or it returns `413` before the node does.
- Fill `config.client-ip.trusted-proxies` with the controller's CIDR so the per-IP rate limit sees the real client IP (`X-Forwarded-For`).
- The `ingress-nginx` project was retired in March 2026; the chart only uses `networking.k8s.io/v1`, which any maintained controller serves. The chart does **not** create `HTTPRoute` (Gateway API).
- `service.type: LoadBalancer` is the alternative without an Ingress.

### S3 backup (doc 33)

Choose **one** destination. With none, there is no backup (`422 backup.not-configured` on the routes).

**Built-in S3 (SeaweedFS)** (lab and environments without a cloud):

```yaml
seaweedfs:
  enabled: true
  bucket: taba
  persistence: { size: 50Gi }
  auth: { existingSecret: "" }   # empty = random secret key, kept across upgrades
```

Creates a `StatefulSet` + `Service` + `Secret` and a Job (`post-install`/`post-upgrade`) that creates the bucket (only if it does not exist yet), like the Compose `seaweedfs-init`. The credentials reach the nodes through `TABA_BACKUP_S3_ACCESS_KEY`/`SECRET_KEY` (Secret), **never** in the ConfigMap. SeaweedFS runs master, volume server, filer and S3 gateway in **one** process, on one node: good for testing the backup, not a redundant production store. Choosing SeaweedFS (and not Garage) is deliberate: the backup needs **atomic conditional writes** ([33-s3-backup.md](33-s3-backup.md)).

**External S3** (AWS, Ceph, your own SeaweedFS or MinIO…; **not** Garage):

```yaml
backup:
  s3:
    bucket: my-bucket
    endpoint: ""              # empty = AWS; outside AWS use the URL (turns path-style on by itself)
    region: us-east-1
    prefix: production        # several clusters can share a bucket with different prefixes
    existingSecret: my-s3     # keys accessKey and secretKey; or inline accessKey/secretKey
```

On AWS prefer **IRSA** (no keys): leave `accessKey`/`secretKey`/`existingSecret` empty and annotate the `serviceAccount` with the role. The role needs `s3:PutObject`, `GetObject`, `DeleteObject` and `ListBucket`. The S3 must accept conditional writes (`If-None-Match`/`If-Match`), which AWS, SeaweedFS and MinIO do (Garage **ignores** the headers and does not work). Do not create a *lifecycle* that deletes what Taba still uses.

Fine tuning (`delay`, `segment.*`, `heartbeat.interval`, `dead-after`, `disk.*`…) goes in `config.backup`. With backup on, the chart's `terminationGracePeriodSeconds` must exceed `drain.timeout` + `backup.drain.timeout`.

### Metrics: collector (`collector.type`)

The node exposes `GET /metrics` on the operations port (one target per node, no aggregation) and optionally pushes OTLP. The chart offers four modes:

| `collector.type` | What it does |
|---|---|
| `none` (default) | Only exposes: pods carry `prometheus.io/scrape`, `/port` and `/path` for whoever already has a collector |
| `prometheus-operator` | Creates a `ServiceMonitor` for **your** Prometheus (only if the `monitoring.coreos.com/v1` CRD exists; `collector.serviceMonitor.requireCrd=false` forces it). Set `labels` to match your Prometheus `serviceMonitorSelector` |
| `prometheus` | Runs a Prometheus (`Deployment`, pod discovery **only in the release namespace**, read-only RBAC for pods). `collector.prometheus.remoteWrite`, `extraScrapeConfigs`, `retention` and `persistence` configure the rest |
| `telegraf` | Runs a Telegraf with `inputs.prometheus` over the annotated pods. `outputs` are **free TOML** in `collector.telegraf.outputs` (default: `outputs.prometheus_client` on `:9273`); output secrets through `collector.telegraf.envFromSecret` |

Do not `sum` the `taba.cluster.*` metrics (each node reports the cluster view; use `max`/`min`). See [13-observability.md](13-observability.md).

**OTLP push** is independent of the collector: `otel.endpoint` turns it on (`OTEL_EXPORTER_OTLP_ENDPOINT`), with `protocol`, `headers` (or `headersSecret`, preferable, since they may be secret), `certificateConfigMap` (PEM CA), `metricExportInterval` (ms), `tracesSampler`, `tracesSamplerArg`, `serviceName` and `resourceAttributes`. A collector that is down does not take the node down.

### Autoscaling with KEDA

Off by default. Needs **KEDA** installed and a **Prometheus** that sees the nodes (`autoscaling.prometheus.serverAddress`, or `collector.type=prometheus`, which already points to the chart's Prometheus).

```yaml
autoscaling:
  enabled: true
  minReplicas: 3        # never below max(3, replication.factor)
  maxReplicas: 6
  prometheus: { serverAddress: http://prometheus.monitoring:9090 }
  disk:     { enabled: true, threshold: 70 }         # % of volume usage, the worst node
  requests: { enabled: true, perNodeRps: 500 }       # requests/s per node
```

| Trigger | Query (default) | Scales when |
|---|---|---|
| `disk` | `max(clamp_min(100 × (1 − kubelet_volume_stats_available_bytes / kubelet_volume_stats_capacity_bytes), 0))` over the `data-<release>-N` PVCs (or `taba_storage_disk_free_bytes` with `disk.source=app` + `capacityBytes`) | the worst volume goes above `disk.threshold` % (default 70, below `backup.disk.evict-above` of 85) |
| `requests` | `sum(rate(http_server_request_duration_seconds_count{server_port="8080"}[2m])) or vector(0)` | the total divided by nodes goes above `requests.perNodeRps` |

Either trigger asks for more nodes; it **only scales down** when all are below their limit. The queries can be replaced whole (`disk.query`, `requests.query`).

**Why it is safe** (each item is verified in [35-helm-use-cases.md](35-helm-use-cases.md#uc-12)):

- **Floor.** `minReplicas` never below `max(3, replication.factor)` (the chart refuses).
- **One node at a time.** `behavior.scaleDown`: 1 pod every 600 s, with a 900 s stabilization window; `scaleUp`: 1 pod every 300 s (joins to the cluster are serialized, [23-operations.md](23-operations.md)).
- **No message lost.** The StatefulSet always removes the **highest ordinal** (never `taba-0`, the first node). The pod gets SIGTERM, the node does the **drain** (re-sends what it holds to other nodes, and to S3 if there is a backup) and only then exits; `terminationGracePeriodSeconds` guarantees that time. `drain.on-stop` stays on (default).
- **Never shrinks a sick cluster.** The queries carry `and on() (min(taba_cluster_quorum_available) == 1)` (`autoscaling.healthGuard`): with no quorum, or without the metric, the query returns no value, `ignoreNullValues: "false"` makes KEDA treat it as an error and the HPA **neither scales up nor down**.
- **Volume.** `persistence.retentionPolicy.whenScaled: Delete` deletes the removed node's volume (it would come back as a new node anyway).
- **Pause.** `autoscaling.paused: true` + `pausedReplicas: N` freezes the replica count (annotation `autoscaling.keda.sh/paused-replicas`), useful during an upgrade or an investigation. `autoscaling.fallback` sets fixed replicas if Prometheus goes down.
- With autoscaling on, the chart **omits** the StatefulSet's `spec.replicas`, so `helm upgrade` does not fight the HPA.

## Install, upgrade, uninstall

```bash
helm upgrade --install taba deploy/helm/taba -n taba --create-namespace -f my-values.yaml --wait --timeout 10m
helm test taba -n taba                        # /health/ready of a node and /metrics through the -ops Service
helm uninstall taba -n taba                   # keeps the taba-secrets Secret and the node volumes
```

- `helm upgrade` replaces the nodes **one at a time**, waiting for `ready` ([23-operations.md](23-operations.md)); the cluster Secret is **not** regenerated.
- To change the Taba version just `--set image.tag=...`; all nodes need the same version.
- To delete everything, secrets and data included: `helm uninstall`, then `kubectl delete secret taba-secrets` and `kubectl delete pvc -l app.kubernetes.io/instance=taba`.

## Known pitfalls

- **`TABA_*` in the pod environment.** Hence `enableServiceLinks: false`; a Service named `taba` would inject `TABA_PORT` and others.
- **Never take two nodes down on purpose.** A Kubernetes node drain respects the PDB (1 at a time); wait for `ready` between one and the next.
- **Total loss.** If all nodes and volumes are lost, the restore comes from S3 by itself at startup ([33-s3-backup.md](33-s3-backup.md)); without a backup there is nothing to restore.
- **The `taba-secrets` Secret is not part of the release.** Deleting it while the cluster is up breaks certificate and token reload; restore it before restarting the nodes.
- **Garage does not work as the backup S3.** It ignores `If-None-Match`/`If-Match` (in versions 2.1.0 and 2.4.1 "create only if absent" overwrites), and without them two nodes may restore at the same time.
- **MinIO.** The project stopped publishing images to Docker Hub and Quay; that is why the chart's built-in S3 is SeaweedFS. If you use MinIO as an external S3, the image is up to you (mirrored in your registry).
- **SeaweedFS volumes.** Each SeaweedFS volume pre-allocates space (`seaweedfs.volumeSizeLimitMB`, default 1024 MB, up to `maxVolumes` volumes): size it for the disk.

## Sources and evidence

- `deploy/helm/taba/` (chart), `values.schema.json` and the validations in `templates/_helpers.tpl`.
- Tests in `deploy/helm/tests/`: `00-static.sh` (lint, rendering of every `ci/*.yaml`, assertions and negative cases, no cluster) and one script per use case (`uc-NN-*.sh`, on a [kind](https://kind.sigs.k8s.io) cluster). The case → test table is in [35-helm-use-cases.md](35-helm-use-cases.md).
- Node keys: `crates/taba-node/src/config/params/mod.rs` and [22-configuration.md](22-configuration.md). Joining, drain and backup behavior: [23-operations.md](23-operations.md), [33-s3-backup.md](33-s3-backup.md).
