#!/bin/bash
# bootstrap-k3s.sh — Provision a K3s cluster on Hetzner Cloud for hKask.
# Prerequisites: HCLOUD_TOKEN env var, hetzner-k3s binary installed.
# Usage: HCLOUD_TOKEN=xxx ./scripts/bootstrap-k3s.sh
set -euo pipefail

: "${HCLOUD_TOKEN:?Set HCLOUD_TOKEN in environment}"

CLUSTER_NAME="${1:-hkask-prod}"
LOCATION="${2:-nbg1}"
MASTER_TYPE="${3:-cx33}"
WORKER_TYPE="${4:-cx43}"
MASTER_COUNT="${5:-3}"
WORKER_COUNT="${6:-3}"

echo "=== hKask K3s Bootstrap ==="
echo "Cluster:  $CLUSTER_NAME"
echo "Location: $LOCATION"
echo "Masters:  $MASTER_COUNT x $MASTER_TYPE"
echo "Workers:  $WORKER_COUNT x $WORKER_TYPE"
echo ""

# ── 1. Create K3s cluster ──────────────────────────────────
echo "Creating K3s cluster (2-3 minutes)..."
hetzner-k3s create \
  --name "$CLUSTER_NAME" \
  --location "$LOCATION" \
  --masters "$MASTER_COUNT" --master-type "$MASTER_TYPE" \
  --workers "$WORKER_COUNT" --worker-type "$WORKER_TYPE" \
  --network-zone eu-central \
  --autoscaling-enabled

export KUBECONFIG="$(pwd)/kubeconfig"
echo "KUBECONFIG: $KUBECONFIG"
echo ""

# ── 2. Verify cluster ──────────────────────────────────────
echo "Verifying cluster..."
kubectl get nodes
echo ""

kubectl get storageclass
echo ""

# ── 3. Install cert-manager ────────────────────────────────
echo "Installing cert-manager..."
kubectl apply -f https://github.com/cert-manager/cert-manager/releases/latest/download/cert-manager.yaml
echo ""

# ── 4. Install NGINX Ingress Controller ────────────────────
echo "Installing NGINX Ingress Controller..."
kubectl apply -f https://raw.githubusercontent.com/kubernetes/ingress-nginx/controller-v1.10.0/deploy/static/provider/cloud/deploy.yaml
echo ""

# ── 5. Wait for ingress ────────────────────────────────────
echo "Waiting for ingress controller to be ready..."
kubectl wait --namespace ingress-nginx \
  --for=condition=ready pod \
  --selector=app.kubernetes.io/component=controller \
  --timeout=300s

echo ""
echo "=== Cluster ready ==="
kubectl get svc -n ingress-nginx
echo ""
echo "External IP above is your Load Balancer IP."
echo "Point your DNS A record to that IP."
echo ""
echo "Next: kask deploy init --domain hkask.your-domain.com"
