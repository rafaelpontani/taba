---
title: Helm chart — use cases
summary: The operating modes of the Taba Helm chart (minimal, development, own secrets, Ingress, backup to built-in (SeaweedFS) or external S3, restore, collectors, OTLP, KEDA autoscaling, upgrade, advanced configuration and failures), each with its values, the expected result and the bash tests that prove it.
type: guide
status: stable
verified-on: 2026-10-16 @ 4faa639
sources: [deploy/helm/tests, deploy/helm/taba/ci, deploy/helm/taba/values.yaml]
---

# Helm chart — use cases

> Read first: [34-helm-chart.md](34-helm-chart.md) (the values). Here each **operating mode** of the chart has the values that turn it on, what to expect and the **tests** that prove it. The operating concepts (scale, drain, upgrade) are in [23-operations.md](23-operations.md).

## How to run the tests

The tests are bash scripts in `deploy/helm/tests/`. Each use case **applies the chart** (`helm upgrade --install` with the `deploy/helm/taba/ci/uc-NN-*.yaml` file) in its own namespace and runs one or more tests; each test prints `PASS`, `FAIL` or `SKIP`.

```bash
cd deploy/helm/tests
./run.sh static          # no cluster: lint, rendering and negative cases (helm or Docker)
./run.sh 01 05           # only these use cases
./run.sh                 # everything (takes a while; UC-12 alone takes ~15 min)
```

| Tool | What for |
|---|---|
| `helm` 3.x and `kubectl` | everything (without a local `helm`, `00-static.sh` uses the `alpine/helm` image through Docker) |
| `kind` + Docker | use cases 01–15 create the `taba-helm` cluster by themselves. To use another cluster: `TABA_TEST_CONTEXT=<context>` |
| internet access | downloads images, KEDA (UC-12) and the Prometheus Operator CRD (UC-10) |

Variables: `TABA_TEST_TAG` (tag of the `taba` and `taba-cli` images, default `rc`), `TABA_TEST_LOAD` (local images for `kind load`), `TABA_TEST_KEEP=1` (do not delete the namespace at the end), `TABA_TEST_TIMEOUT`, `TABA_TEST_NO_NETPOL=1` (CNI without NetworkPolicy), `TABA_TEST_INGRESS_URL` (UC-04 with real traffic). **A missing tool or network is `SKIP` (code 77), never `PASS`.** The exit code is 0 with no failures.

`00-static.sh` also covers each case without a cluster: it renders the `ci/*.yaml` files and checks the manifests (`UC-NN` prefix in the test names), besides the chart's refusals (UC-15).

## Map of the cases

| Case | Operating mode | Values (`ci/`) | Script |
|---|---|---|---|
| [UC-01](#uc-01) | Minimal production: 3 nodes, generated secrets | `uc-01-minimo.yaml` | `uc-01-minimo.sh` |
| [UC-02](#uc-02) | Development: 1 node, no auth | `uc-02-desenvolvimento.yaml` | `uc-02-desenvolvimento.sh` |
| [UC-03](#uc-03) | Operator's secrets and token rotation | `uc-03-segredos-existentes.yaml` | `uc-03-segredos-existentes.sh` |
| [UC-04](#uc-04) | Ingress (host, class, TLS) | `uc-04-ingress.yaml` | `uc-04-ingress.sh` |
| [UC-05](#uc-05) | Backup to built-in S3 (SeaweedFS) | `uc-05-backup-seaweedfs.yaml` | `uc-05-backup-seaweedfs.sh` |
| [UC-06](#uc-06) | Backup to external S3 | `uc-06-s3-externo.yaml` | `uc-06-s3-externo.sh` |
| [UC-07](#uc-07) | Restore after losing every node | `uc-05-backup-seaweedfs.yaml` | `uc-07-restore.sh` |
| [UC-08](#uc-08) | The chart's Prometheus collector | `uc-08-coletor-prometheus.yaml` | `uc-08-coletor-prometheus.sh` |
| [UC-09](#uc-09) | The chart's Telegraf collector | `uc-09-coletor-telegraf.yaml` | `uc-09-coletor-telegraf.sh` |
| [UC-10](#uc-10) | Prometheus Operator (ServiceMonitor) | `uc-10-prometheus-operator.yaml` | `uc-10-prometheus-operator.sh` |
| [UC-11](#uc-11) | OTLP push | `uc-11-otlp.yaml` | `uc-11-otlp.sh` |
| [UC-12](#uc-12) | KEDA autoscaling | `uc-12-autoscaling-keda.yaml` | `uc-12-autoscaling-keda.sh` |
| [UC-13](#uc-13) | Rolling upgrade | `uc-13-upgrade.yaml` | `uc-13-upgrade.sh` |
| [UC-14](#uc-14) | Advanced node configuration | `uc-14-config-avancada.yaml` | `uc-14-config-avancada.sh` |
| [UC-15](#uc-15) | Failures and safeguards | `uc-15-falhas.yaml` | `uc-15-falhas.sh` |

---

## UC-01

**Minimal production.** Three nodes; the pre-install Job runs `taba init` and creates the Secret; everything else at the default.

```bash
helm install taba deploy/helm/taba -n taba --create-namespace
```

**Expected:** three `ready` pods, `/health/cluster` with no conditions, a 32+ character token, the API refusing a call without a token (`401`), publish and consume working, the API Service publishing only 8080, `helm uninstall` keeping the Secret.

| Test | What it proves |
|---|---|
| UC-01.1 – 01.3 | Three pods ready, cluster healthy, `/health/ready` on each node |
| UC-01.4 – 01.5 | Secret with `root-token`, `tls.crt`, `tls.key`; long token |
| UC-01.6 | PDB allows 1 unavailable |
| UC-01.7 | `helm test` (readiness and `/metrics` through the `-ops` Service) |
| UC-01.8 | API without a token → `401` |
| UC-01.9 – 01.10 | Publish 10 and consume 10 |
| UC-01.11 | The API Service only exposes 8080 |
| UC-01.12 | Secret survives `helm uninstall` |

## UC-02

**Development.** `config.mode: development`: one node, no authentication, no cluster TLS, no Secret and no Job. For a lab and application integration tests.

**Expected:** one pod; no Secret or Job; even with `replicaCount: 3` the StatefulSet stays at 1; publishing without a token works.

| Test | What it proves |
|---|---|
| UC-02.1 – 02.3 | One pod ready, no Secret and no init Job |
| UC-02.4 | `/health/ready` |
| UC-02.5 | `replicas: 1` despite `replicaCount: 3` |
| UC-02.6 – 02.7 | Publish and consume without a token |

## UC-03

**Operator's secrets.** `secrets.existingSecret` points to a Secret you control (a certificate from your own CA, a token from a vault). The chart generates nothing. The token is changed to **two lines** (new, previous) without restarting the nodes.

**Expected:** the operator's token is accepted, a wrong one is not, after rotation both the new and the previous work, no pod restarts.

| Test | What it proves |
|---|---|
| UC-03.0 | The test generates the pair with `taba init` and creates the Secret |
| UC-03.1 – 03.3 | Three healthy nodes; the chart created no Secret or Job of its own |
| UC-03.4 – 03.5 | Right token in, wrong token out |
| UC-03.6 | The new token takes effect without a restart (up to 4 min: kubelet + `auth.reload-interval`) |
| UC-03.7 | The previous one keeps working during the rotation |
| UC-03.8 | No restarts |

## UC-04

**Ingress.** `ingress.enabled`, `className`, `annotations`, `hosts`, `tls`. Only the `api` port is a backend.

**Expected:** the `Ingress` object with the class, host and TLS; the backend is the Service's `api` port, with endpoints; the API Service does not publish 8081 or 9090. With `TABA_TEST_INGRESS_URL` (a controller in the cluster), the API answers through the Ingress and `/metrics` does not.

| Test | What it proves |
|---|---|
| UC-04.1 – 04.4 | Nodes ready; class, host, TLS and annotation applied |
| UC-04.5 – 04.6 | `api` backend, with endpoints |
| UC-04.7 | 8081 and 9090 outside the Service |
| UC-04.8 | API through the Ingress (**SKIP** without a controller) |
| UC-04.9 | `/metrics` does not pass through the Ingress |

## UC-05

**Backup to built-in S3 (SeaweedFS).** `seaweedfs.enabled: true`. The Job creates the bucket; credentials go through a Secret; the backup turns on by itself.

**Expected:** SeaweedFS ready, bucket `taba` created, `GET /node/backup` shows the backup configured, objects appear in the bucket after `flush`.

| Test | What it proves |
|---|---|
| UC-05.1 – 05.3 | SeaweedFS, three nodes and cluster ready |
| UC-05.4 | Bucket created by the Job |
| UC-05.5 | The node reaches S3 (`s3Reachable`) |
| UC-05.6 | Credentials outside the ConfigMap |
| UC-05.7 – 05.9 | Publish 20, `flush`, objects in the bucket |
| UC-05.10 | The S3 requires credentials (anonymous access refused) |

## UC-06

**Backup to external S3.** `backup.s3.*` with endpoint, prefix and keys in a Secret. The test installs a SeaweedFS in **another release** (`storage`) and points to it; on AWS the equivalent is IRSA (no keys), verified only in rendering (`00-static.sh`).

**Expected:** no built-in S3 in the cluster's release; the keys come from the chart's Secret and are not in the ConfigMap; objects appear under the configured prefix.

| Test | What it proves |
|---|---|
| UC-06.1 – 06.2 | Nodes ready and healthy |
| UC-06.3 | No S3 of its own |
| UC-06.4 – 06.5 | Keys in a Secret, outside the ConfigMap |
| UC-06.6 – 06.8 | Publish 20, `flush`, objects under `cluster-a/` |

## UC-07

**Restore.** With backup to SeaweedFS: planned stop (`POST /cluster/stop`), StatefulSet to 0 (the node volumes are deleted by `whenScaled: Delete`), back to 3. The first node to take the lock restores definitions, tokens and messages ([33-s3-backup.md](33-s3-backup.md)).

**Expected:** the cluster comes back healthy **with the messages**.

| Test | What it proves |
|---|---|
| UC-07.1 – 07.2 | Cluster healthy, 15 messages published |
| UC-07.3 – 07.4 | Cluster `flush` and `cluster/stop` (root token) |
| UC-07.5 | Node volumes deleted |
| UC-07.6 – 07.7 | Three nodes come back and the cluster is healthy |
| UC-07.8 | The 15 messages came back from the bucket |

## UC-08

**The chart's Prometheus collector.** `collector.type: prometheus`: a Prometheus that discovers the release's pods (only in the namespace) and scrapes `/metrics` from the operations port. `remoteWrite` sends to a long-term backend.

| Test | What it proves |
|---|---|
| UC-08.1 – 08.2 | Nodes and Prometheus ready |
| UC-08.3 | Three `up` targets |
| UC-08.4 – 08.5 | `taba_cluster_members` arrives, with `namespace` and `pod` labels |
| UC-08.6 | Pods annotated with `prometheus.io/*` |
| UC-08.7 | RBAC for pods only |

## UC-09

**The chart's Telegraf collector.** `collector.type: telegraf`: reads `/metrics` from the annotated pods and re-exposes it on `:9273` (default) or sends it to the TOML `outputs` you write (InfluxDB, Kafka, file…).

| Test | What it proves |
|---|---|
| UC-09.1 – 09.2 | Nodes and Telegraf ready |
| UC-09.3 – 09.4 | Metrics on `:9273`, from three pods |
| UC-09.5 | No permission errors |
| UC-09.6 | Replaceable `outputs` (here, `file` to stdout) |

## UC-10

**Prometheus Operator.** `collector.type: prometheus-operator`: the chart creates a `ServiceMonitor` for **your** Prometheus, only if the CRD exists.

| Test | What it proves |
|---|---|
| UC-10.1 | Without the CRD the chart installs and does not create the `ServiceMonitor` |
| UC-10.2 – 10.4 | With the CRD: created, with your selector's `labels`, `ops` port and interval |
| UC-10.5 – 10.6 | The selector matches the `-ops` Service, which has 3 endpoints |
| UC-10.7 | No `prometheus.io` annotations (the operator ignores them) |

## UC-11

**OTLP push.** `otel.*` become `OTEL_*` on the pod. The test runs an OpenTelemetry Collector and checks the metrics arriving; `/metrics` keeps working and the node does not depend on the collector.

| Test | What it proves |
|---|---|
| UC-11.1 – 11.3 | Nodes ready; `OTEL_*` on the pod |
| UC-11.4 – 11.5 | The collector receives metrics with the configured `service.name` |
| UC-11.6 | `/metrics` (pull) intact |
| UC-11.7 | A collector that is down does not take the nodes down |

## UC-12

**KEDA autoscaling.** See [34-helm-chart.md](34-helm-chart.md#autoscaling-with-keda). The test installs KEDA (`TABA_TEST_KEDA_VERSION`, default 2.17.2) and uses the chart's Prometheus.

**Expected:** floor of 3; traffic with a low `perNodeRps` takes it to 4; when the target is restored, **one node at a time** back to 3, the cluster healthy and **all messages still there**.

| Test | What it proves |
|---|---|
| UC-12.1 – 12.5 | Floor of 3; `ScaledObject` ready; KEDA's HPA with scale-down of 1 pod; cluster healthy |
| UC-12.6 – 12.7 | Scale-up by request volume to 4 nodes; cluster healthy with 4 |
| UC-12.8 – 12.9 | Shrinks to the floor and **never below 3** |
| UC-12.10 | Cluster healthy afterwards |
| UC-12.11 | The 20 messages published before survive (the node drained) |
| UC-12.12 | The removed node's volume was deleted |
| UC-12.13 | `autoscaling.paused` applies the pause annotation |

The **disk** trigger is only proven by rendering (`00-static.sh`: `kubelet_volume_stats_*` or `taba_storage_disk_free_bytes` query), because filling a real volume is slow and depends on the `StorageClass`.

## UC-13

**Rolling upgrade.** `helm upgrade` with a `config` change (checksum on the pod) restarts the nodes **one at a time**.

**Expected:** never fewer than 2 nodes ready during the upgrade, new configuration on all of them, **14 of 14** messages afterwards ([23-operations.md](23-operations.md)), cluster Secret intact.

| Test | What it proves |
|---|---|
| UC-13.1 – 13.3 | Healthy cluster with 14 messages |
| UC-13.4 – 13.5 | `helm upgrade` finished and the StatefulSet changed revision |
| UC-13.6 | Never fewer than 2 nodes ready |
| UC-13.7 – 13.8 | Three ready and healthy afterwards |
| UC-13.9 | New configuration on every node |
| UC-13.10 | 14/14 messages |
| UC-13.11 | Secret not regenerated |

## UC-14

**Advanced configuration.** Many `config.*` keys at once, `extraEnv`, rate limit, `trusted-proxies`, `allowed-ips`. Since the node refuses an unknown key, coming up ready already proves the keys exist.

| Test | What it proves |
|---|---|
| UC-14.1 – 14.2 | The nodes accepted all the keys |
| UC-14.3 – 14.5 | ConfigMap, `allowed-ips` and `extraEnv` arrived |
| UC-14.6 | JSON logs at `debug` level |
| UC-14.7 – 14.8 | A message within `message.max-bytes` passes; above it is refused |
| UC-14.9 | Operations reachable from inside the allowed CIDR |
| UC-14.10 – 14.11 | `helm upgrade --set` changes the configuration and the cluster comes back healthy |

## UC-15

**Failures and safeguards.** `NetworkPolicy`, PDB and recovery.

| Test | What it proves |
|---|---|
| UC-15.1 – 15.2 | Three healthy nodes; PDB allows 1 unavailable |
| UC-15.3 – 15.4 | An outside pod does **not** reach 9090 or 8081 (**SKIP** with `TABA_TEST_NO_NETPOL=1`) |
| UC-15.5 | The outside pod reaches the API (8080) |
| UC-15.6 | A node dies and rejoins (3 ready, no conditions) |
| UC-15.7 | `taba-0` (first-node) dies and comes back as a normal node, without forming a new cluster |
| UC-15.8 | Two nodes die together: no quorum for a moment, then it recovers |
| UC-15.9 | The 14 messages are still there |

The **chart's refusals** (`replicaCount` below the minimum, short grace period, repeated ports, invalid `quorum`, `evict-*` out of order, built-in S3 + external S3, unpaired key, values outside the enum, unsafe autoscaling) are tested in `00-static.sh`, under the `UC-15` prefix.

---

## Choosing the mode

| I want… | Use |
|---|---|
| To try an application | UC-02 |
| A real, minimal cluster | UC-01 |
| To expose the API outside the cluster | UC-04 (and `trusted-proxies` from [UC-14](#uc-14)) |
| Backup without a cloud | UC-05 (and restore as in UC-07) |
| Backup in the cloud | UC-06 |
| Metrics in a stack I already have | UC-10 (Operator) or `collector.type: none` + annotations |
| Metrics without a stack | UC-08 (Prometheus) or UC-09 (Telegraf) |
| Metrics to an OpenTelemetry Collector | UC-11 |
| Capacity that follows the load | UC-12 |

## Sources and evidence

- Scripts and test values: `deploy/helm/tests/` (`lib.sh`, `run.sh`, `00-static.sh`, `uc-*.sh`) and `deploy/helm/taba/ci/`.
- A use case is "proven" when its script ends with `0 failures`; `SKIP`s stay explicit in the output and in the `run.sh` summary.
- Recorded run (2026-10-16, `kind` 0.30 with Kubernetes 1.34, `rafaelpontani/taba:rc` and `taba-cli:rc` images, KEDA 2.17.2): `00-static.sh` 103 tests and UC-01 to UC-15 with no failures; the only `SKIP` is UC-04.8 (no Ingress controller in kind). Nothing has been exercised yet on a production cluster, with another CNI or with real full disks (KEDA's disk trigger is proven by rendering only).
