# 📄 Level 3 — Enterprise Deployment (Kubernetes)

> **Fully decoupled microservices: stateless gateway pods + auto-scaling GPU inference pools.**

This guide covers deploying Isartor on Kubernetes with Helm, horizontal pod autoscaling, dedicated GPU inference pools (vLLM or TGI), service mesh integration, and production-grade observability.

---

## When to Use Level 3

| ✅ Good Fit | ❌ Overkill For |
| --- | --- |
| 100+ concurrent users | < 50 users → Level 2 Docker Compose |
| Multi-region / multi-zone HA | Single-machine development → Level 1 |
| Auto-scaling GPU inference | No GPU budget → Level 1 embedded candle |
| Compliance: mTLS, audit logs, RBAC | Hobby projects / PoCs |
| Cost optimisation via scale-to-zero | Teams without Kubernetes experience |

---

## Architecture

```
                        ┌────────────────────┐
                        │    Ingress / ALB    │
                        │  (TLS termination)  │
                        └──────────┬─────────┘
                                   │
                    ┌──────────────┴──────────────┐
                    │      Gateway Deployment      │
                    │      (N stateless pods)       │
                    │                              │
                    │  ┌────────┐   ┌────────┐    │
                    │  │ Pod 1  │   │ Pod N  │    │
                    │  │isartor │   │isartor │    │
                    │  └────────┘   └────────┘    │
                    │                              │
                    │  HPA: CPU / custom metrics   │
                    └──────────────┬───────────────┘
                                   │
                          Internal ClusterIP
                                   │
              ┌────────────────────┼────────────────────┐
              │                    │                     │
     ┌────────▼───────┐  ┌────────▼───────┐   ┌────────▼───────┐
     │ Inference Pool  │  │ Embedding Pool  │   │ Cloud LLM      │
     │ (vLLM / TGI)   │  │ (TEI / llama)   │   │ (OpenAI / etc) │
     │                 │  │                 │   │ (Layer 3 only)  │
     │ GPU Nodes       │  │ CPU/GPU Nodes   │   └────────────────┘
     │ HPA on GPU util │  │ HPA on RPS      │
     └─────────────────┘  └─────────────────┘
```

### Component Summary

| Component | Replicas | Scaling Metric | Resource |
| --- | --- | --- | --- |
| **Gateway** | 2–20 | CPU utilisation / request rate | CPU nodes |
| **Inference Pool** (vLLM) | 1–N | GPU utilisation / queue depth | GPU nodes |
| **Embedding Pool** (TEI) | 1–N | Requests per second | CPU or GPU nodes |
| **OTel Collector** | 1 (DaemonSet or Deployment) | — | CPU nodes |
| **Ingress Controller** | 1–2 | — | CPU nodes |

---

## Prerequisites

| Requirement | Details |
| --- | --- |
| **Kubernetes cluster** | 1.28+ (EKS, GKE, AKS, or bare metal) |
| **Helm** | v3.12+ |
| **kubectl** | Matching cluster version |
| **GPU nodes** (for inference pool) | NVIDIA GPU Operator installed, or GKE/EKS GPU node pools |
| **Container registry** | For pushing the Isartor gateway image |
| **Ingress controller** | nginx-ingress, Istio, or cloud ALB |

---

## Step 1: Build & Push the Gateway Image

```bash
# Build
docker build -t your-registry.io/isartor:v0.1.0 -f docker/Dockerfile .

# Push
docker push your-registry.io/isartor:v0.1.0
```

---

## Step 2: Namespace & Secrets

```bash
kubectl create namespace isartor

# Cloud LLM API key (Layer 3 fallback)
kubectl create secret generic isartor-llm-secret \
  --namespace isartor \
  --from-literal=api-key='sk-...'

# Gateway API key (Layer 0 auth)
kubectl create secret generic isartor-gateway-secret \
  --namespace isartor \
  --from-literal=gateway-api-key='your-production-key'
```

---

## Step 3: Gateway Deployment

```yaml
# k8s/gateway-deployment.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: isartor-gateway
  namespace: isartor
  labels:
    app: isartor-gateway
spec:
  replicas: 2
  selector:
    matchLabels:
      app: isartor-gateway
  template:
    metadata:
      labels:
        app: isartor-gateway
    spec:
      containers:
        - name: gateway
          image: your-registry.io/isartor:v0.1.0
          ports:
            - containerPort: 8080
              name: http
          env:
            - name: ISARTOR_HOST_PORT
              value: "0.0.0.0:8080"
            - name: ISARTOR_GATEWAY_API_KEY
              valueFrom:
                secretKeyRef:
                  name: isartor-gateway-secret
                  key: gateway-api-key
            # Cache
            - name: ISARTOR_CACHE_MODE
              value: "both"
            - name: ISARTOR_SIMILARITY_THRESHOLD
              value: "0.85"
            - name: ISARTOR_CACHE_TTL_SECS
              value: "300"
            - name: ISARTOR_CACHE_MAX_CAPACITY
              value: "50000"
            # Inference pool (internal service)
            - name: ISARTOR_LAYER2__SIDECAR_URL
              value: "http://isartor-inference:8081"
            - name: ISARTOR_LAYER2__MODEL_NAME
              value: "phi-3-mini"
            - name: ISARTOR_LAYER2__TIMEOUT_SECONDS
              value: "30"
            # Embedding pool (internal service)
            - name: ISARTOR_EMBEDDING_SIDECAR__SIDECAR_URL
              value: "http://isartor-embedding:8082"
            - name: ISARTOR_EMBEDDING_SIDECAR__MODEL_NAME
              value: "all-minilm"
            - name: ISARTOR_EMBEDDING_SIDECAR__TIMEOUT_SECONDS
              value: "10"
            # Layer 3 — Cloud LLM
            - name: ISARTOR_LLM_PROVIDER
              value: "openai"
            - name: ISARTOR_EXTERNAL_LLM_MODEL
              value: "gpt-4o-mini"
            - name: ISARTOR_EXTERNAL_LLM_API_KEY
              valueFrom:
                secretKeyRef:
                  name: isartor-llm-secret
                  key: api-key
            # Observability
            - name: ISARTOR_ENABLE_MONITORING
              value: "true"
            - name: ISARTOR_OTEL_EXPORTER_ENDPOINT
              value: "http://otel-collector.isartor:4317"
            # Pipeline v2 tuning
            - name: ISARTOR_PIPELINE_EMBEDDING_DIM
              value: "384"
            - name: ISARTOR_PIPELINE_SIMILARITY_THRESHOLD
              value: "0.92"
            - name: ISARTOR_PIPELINE_RERANK_TOP_K
              value: "5"
            - name: ISARTOR_PIPELINE_MAX_CONCURRENCY
              value: "512"
            - name: ISARTOR_PIPELINE_MIN_CONCURRENCY
              value: "8"
            - name: ISARTOR_PIPELINE_TARGET_LATENCY_MS
              value: "300"
          resources:
            requests:
              cpu: "250m"
              memory: "128Mi"
            limits:
              cpu: "1000m"
              memory: "256Mi"
          readinessProbe:
            httpGet:
              path: /healthz
              port: http
            initialDelaySeconds: 5
            periodSeconds: 10
          livenessProbe:
            httpGet:
              path: /healthz
              port: http
            initialDelaySeconds: 10
            periodSeconds: 30
---
apiVersion: v1
kind: Service
metadata:
  name: isartor-gateway
  namespace: isartor
spec:
  selector:
    app: isartor-gateway
  ports:
    - port: 8080
      targetPort: http
      name: http
  type: ClusterIP
```

---

## Step 4: Inference Pool (vLLM)

[vLLM](https://github.com/vllm-project/vllm) provides high-throughput, GPU-optimised inference with continuous batching.

```yaml
# k8s/inference-deployment.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: isartor-inference
  namespace: isartor
  labels:
    app: isartor-inference
spec:
  replicas: 1
  selector:
    matchLabels:
      app: isartor-inference
  template:
    metadata:
      labels:
        app: isartor-inference
    spec:
      containers:
        - name: vllm
          image: vllm/vllm-openai:latest
          args:
            - "--model"
            - "microsoft/Phi-3-mini-4k-instruct"
            - "--host"
            - "0.0.0.0"
            - "--port"
            - "8081"
            - "--max-model-len"
            - "4096"
            - "--gpu-memory-utilization"
            - "0.9"
          ports:
            - containerPort: 8081
              name: http
          resources:
            requests:
              nvidia.com/gpu: 1
              memory: "8Gi"
            limits:
              nvidia.com/gpu: 1
              memory: "16Gi"
          readinessProbe:
            httpGet:
              path: /health
              port: http
            initialDelaySeconds: 60
            periodSeconds: 10
      nodeSelector:
        nvidia.com/gpu.present: "true"
      tolerations:
        - key: nvidia.com/gpu
          operator: Exists
          effect: NoSchedule
---
apiVersion: v1
kind: Service
metadata:
  name: isartor-inference
  namespace: isartor
spec:
  selector:
    app: isartor-inference
  ports:
    - port: 8081
      targetPort: http
      name: http
  type: ClusterIP
```

### Alternative: Text Generation Inference (TGI)

Replace vLLM with [TGI](https://github.com/huggingface/text-generation-inference) if you prefer Hugging Face's inference server:

```yaml
containers:
  - name: tgi
    image: ghcr.io/huggingface/text-generation-inference:latest
    args:
      - "--model-id"
      - "microsoft/Phi-3-mini-4k-instruct"
      - "--port"
      - "8081"
      - "--max-input-length"
      - "4096"
      - "--max-total-tokens"
      - "8192"
```

### Alternative: llama.cpp Server (CPU / Light GPU)

For budget clusters without heavy GPU nodes:

```yaml
containers:
  - name: llama-cpp
    image: ghcr.io/ggml-org/llama.cpp:server
    args:
      - "--host"
      - "0.0.0.0"
      - "--port"
      - "8081"
      - "--hf-repo"
      - "microsoft/Phi-3-mini-4k-instruct-gguf"
      - "--hf-file"
      - "Phi-3-mini-4k-instruct-q4.gguf"
      - "--ctx-size"
      - "4096"
      - "--n-gpu-layers"
      - "99"
```

---

## Step 5: Embedding Pool (TEI)

[Text Embeddings Inference (TEI)](https://github.com/huggingface/text-embeddings-inference) provides optimised embedding generation.

```yaml
# k8s/embedding-deployment.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: isartor-embedding
  namespace: isartor
  labels:
    app: isartor-embedding
spec:
  replicas: 2
  selector:
    matchLabels:
      app: isartor-embedding
  template:
    metadata:
      labels:
        app: isartor-embedding
    spec:
      containers:
        - name: tei
          image: ghcr.io/huggingface/text-embeddings-inference:cpu-latest
          args:
            - "--model-id"
            - "sentence-transformers/all-MiniLM-L6-v2"
            - "--port"
            - "8082"
          ports:
            - containerPort: 8082
              name: http
          resources:
            requests:
              cpu: "500m"
              memory: "512Mi"
            limits:
              cpu: "2000m"
              memory: "1Gi"
          readinessProbe:
            httpGet:
              path: /health
              port: http
            initialDelaySeconds: 30
            periodSeconds: 10
---
apiVersion: v1
kind: Service
metadata:
  name: isartor-embedding
  namespace: isartor
spec:
  selector:
    app: isartor-embedding
  ports:
    - port: 8082
      targetPort: http
      name: http
  type: ClusterIP
```

---

## Step 6: Horizontal Pod Autoscaler

### Gateway HPA

```yaml
# k8s/gateway-hpa.yaml
apiVersion: autoscaling/v2
kind: HorizontalPodAutoscaler
metadata:
  name: isartor-gateway-hpa
  namespace: isartor
spec:
  scaleTargetRef:
    apiVersion: apps/v1
    kind: Deployment
    name: isartor-gateway
  minReplicas: 2
  maxReplicas: 20
  metrics:
    - type: Resource
      resource:
        name: cpu
        target:
          type: Utilization
          averageUtilization: 70
  behavior:
    scaleUp:
      stabilizationWindowSeconds: 30
      policies:
        - type: Pods
          value: 4
          periodSeconds: 60
    scaleDown:
      stabilizationWindowSeconds: 300
      policies:
        - type: Pods
          value: 2
          periodSeconds: 120
```

### Inference Pool HPA (Custom Metrics)

For GPU-based scaling, use custom metrics from Prometheus:

```yaml
# k8s/inference-hpa.yaml
apiVersion: autoscaling/v2
kind: HorizontalPodAutoscaler
metadata:
  name: isartor-inference-hpa
  namespace: isartor
spec:
  scaleTargetRef:
    apiVersion: apps/v1
    kind: Deployment
    name: isartor-inference
  minReplicas: 1
  maxReplicas: 8
  metrics:
    - type: Pods
      pods:
        metric:
          name: gpu_utilization
        target:
          type: AverageValue
          averageValue: "80"
```

> **Note:** GPU-based HPA requires the [Prometheus Adapter](https://github.com/kubernetes-sigs/prometheus-adapter) or KEDA to expose GPU metrics to the HPA controller.

---

## Step 7: Ingress

### nginx-ingress Example

```yaml
# k8s/ingress.yaml
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: isartor-ingress
  namespace: isartor
  annotations:
    nginx.ingress.kubernetes.io/proxy-body-size: "10m"
    nginx.ingress.kubernetes.io/proxy-read-timeout: "120"
    cert-manager.io/cluster-issuer: "letsencrypt-prod"
spec:
  ingressClassName: nginx
  tls:
    - hosts:
        - api.isartor.example.com
      secretName: isartor-tls
  rules:
    - host: api.isartor.example.com
      http:
        paths:
          - path: /
            pathType: Prefix
            backend:
              service:
                name: isartor-gateway
                port:
                  number: 8080
```

### Istio VirtualService (Service Mesh)

```yaml
apiVersion: networking.istio.io/v1beta1
kind: VirtualService
metadata:
  name: isartor-vs
  namespace: isartor
spec:
  hosts:
    - api.isartor.example.com
  gateways:
    - isartor-gateway
  http:
    - match:
        - uri:
            prefix: /api/
      route:
        - destination:
            host: isartor-gateway
            port:
              number: 8080
      timeout: 120s
      retries:
        attempts: 2
        perTryTimeout: 60s
```

---

## Step 8: Apply Everything

```bash
# Apply in order
kubectl apply -f k8s/gateway-deployment.yaml
kubectl apply -f k8s/inference-deployment.yaml
kubectl apply -f k8s/embedding-deployment.yaml
kubectl apply -f k8s/gateway-hpa.yaml
kubectl apply -f k8s/inference-hpa.yaml
kubectl apply -f k8s/ingress.yaml

# Verify
kubectl get pods -n isartor
kubectl get svc -n isartor
kubectl get hpa -n isartor
```

---

## Observability in Level 3

For Kubernetes deployments, you have several options:

| Approach | Stack | Effort |
| --- | --- | --- |
| **Self-managed** | OTel Collector DaemonSet → Jaeger + Prometheus + Grafana | Medium |
| **Managed (AWS)** | AWS X-Ray + CloudWatch + Managed Grafana | Low |
| **Managed (GCP)** | Cloud Trace + Cloud Monitoring | Low |
| **Managed (Azure)** | Azure Monitor + Application Insights | Low |
| **Third-party** | Datadog / New Relic / Grafana Cloud | Low |

The gateway exports traces and metrics via OTLP gRPC to whatever `ISARTOR_OTEL_EXPORTER_ENDPOINT` points at. See [`docs/observability.md`](observability.md) for detailed setup.

---

## Cost Optimisation

| Strategy | Description |
| --- | --- |
| **Spot / preemptible nodes** | Use for inference pods (they're stateless and restart quickly) |
| **Scale-to-zero** | Use KEDA with queue-depth trigger to scale inference to 0 when idle |
| **Right-size GPU** | A100 80 GB for large models, T4/L4 for Phi-3-mini (4 GB VRAM is sufficient) |
| **Shared GPU** | NVIDIA MPS or MIG to run multiple inference pods per GPU |
| **Semantic cache** | Higher `ISARTOR_CACHE_MAX_CAPACITY` = fewer inference calls |
| **Smaller quantisation** | Q4_K_M uses less VRAM at marginal quality cost |

---

## Security Checklist

- [ ] TLS termination at ingress (cert-manager + Let's Encrypt or cloud certs)
- [ ] mTLS between services (Istio / Linkerd / Cilium)
- [ ] `ISARTOR_GATEWAY_API_KEY` from Kubernetes Secret, not plaintext
- [ ] `ISARTOR_EXTERNAL_LLM_API_KEY` from Kubernetes Secret
- [ ] Network policies restricting pod-to-pod communication
- [ ] RBAC: least-privilege ServiceAccounts for each workload
- [ ] Pod security standards: `restricted` or `baseline`
- [ ] Image scanning (Trivy, Snyk) in CI pipeline
- [ ] Audit logging enabled on the cluster

---

## Downgrading to Level 2

If Kubernetes overhead doesn't justify the scale:

1. Export your env vars from the Kubernetes ConfigMap/Secret.
2. Map them into `docker/.env.full`.
3. Run `docker compose -f docker-compose.full.yml up --build`.

No code changes — the binary is identical across all three tiers.

---

*← Back to [README](../README.md)*
