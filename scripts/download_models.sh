#!/usr/bin/env bash
# =============================================================================
# scripts/download_models.sh
#
# Downloads the required Isartor models to ./models/ for offline / air-gapped
# deployments.  Called during Docker build when BUNDLE_MODELS=true.
#
# Usage:
#   ./scripts/download_models.sh [--offline-bundle]
#
# With --offline-bundle: downloads all models and saves them to ./models/
#   so they are available without internet access at container startup.
#
# At runtime Isartor checks for models in ./models/ (or the path set via
# ISARTOR__EMBEDDED__MODEL_PATH) and skips the HuggingFace Hub download if
# the files are already present.
# =============================================================================

set -euo pipefail

MODELS_DIR="${MODELS_DIR:-./models}"
OFFLINE_BUNDLE=false

for arg in "$@"; do
    case "$arg" in
        --offline-bundle) OFFLINE_BUNDLE=true ;;
        *) echo "Unknown argument: $arg" >&2; exit 1 ;;
    esac
done

if [ "$OFFLINE_BUNDLE" = "true" ]; then
    echo "[download_models.sh] Downloading models for offline bundle..."
    mkdir -p "$MODELS_DIR"

    # ── all-MiniLM-L6-v2 (L1b semantic cache embedding model) ────────
    # Model: sentence-transformers/all-MiniLM-L6-v2
    # Size: ~90 MB
    MINILM_DIR="$MODELS_DIR/sentence-transformers--all-MiniLM-L6-v2"
    if [ ! -f "$MINILM_DIR/config.json" ]; then
        echo "[download_models.sh] Downloading all-MiniLM-L6-v2..."
        mkdir -p "$MINILM_DIR"
        BASE_URL="https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main"
        for file in config.json tokenizer.json tokenizer_config.json special_tokens_map.json \
                    pytorch_model.bin; do
            echo "  → $file"
            curl -fsSL --retry 3 "$BASE_URL/$file" -o "$MINILM_DIR/$file"
        done
        echo "[download_models.sh] ✓ all-MiniLM-L6-v2 downloaded."
    else
        echo "[download_models.sh] ✓ all-MiniLM-L6-v2 already present, skipping."
    fi

    echo "[download_models.sh] All models downloaded to $MODELS_DIR"
    ls -lh "$MODELS_DIR"
else
    echo "[download_models.sh] No --offline-bundle flag. Nothing to do."
    echo "  To pre-bundle models for air-gapped deployments, run:"
    echo "    ./scripts/download_models.sh --offline-bundle"
fi
