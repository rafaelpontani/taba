# Taba Helm chart

Installs a Taba cluster on Kubernetes: `StatefulSet` of nodes, Services (API, cluster entry, headless,
operations), generated cluster secrets, and — optionally — Ingress, backup to built-in SeaweedFS S3 or external S3,
a Prometheus or Telegraf collector (or a `ServiceMonitor`), OTLP push and KEDA autoscaling.

```bash
helm install taba deploy/helm/taba -n taba --create-namespace
helm test taba -n taba
```

- **Every value and how it maps to the node's configuration:** [doc/en-US/34-helm-chart.md](../../../doc/en-US/34-helm-chart.md)
  (pt-BR: [doc/pt-BR/34-chart-helm.md](../../../doc/pt-BR/34-chart-helm.md)).
- **Operating modes and the test for each:** [doc/en-US/35-helm-use-cases.md](../../../doc/en-US/35-helm-use-cases.md).
- **Tests:** `deploy/helm/tests/run.sh static` (no cluster) or `run.sh 01 05` (kind). Values used by the tests
  are in `ci/`, which are also good starting points.

Quick reference:

| I want… | Set |
|---|---|
| A real cluster | defaults (3 nodes) |
| A single dev node | `config.mode=development` |
| To expose the API | `ingress.enabled=true`, `ingress.hosts`, `ingress.tls`, `config.client-ip.trusted-proxies` |
| Backup without a cloud | `seaweedfs.enabled=true` |
| Backup to S3 | `backup.s3.bucket` (+ `existingSecret`, or IRSA through `serviceAccount.annotations`) |
| Metrics | `collector.type=prometheus-operator \| prometheus \| telegraf`, or `otel.endpoint` |
| Autoscaling | `autoscaling.enabled=true` + `autoscaling.prometheus.serverAddress` (needs KEDA) |

Notes: the `ingress-nginx` project was retired in March 2026 — use any maintained Ingress controller; the chart
only needs `networking.k8s.io/v1`. Ports 8081 (cluster, mTLS) and 9090 (operations, no authentication) must
never be exposed outside the cluster.
