#!/usr/bin/env bash
# Add HNSW ANN dependencies to Cargo.toml
set -e

# Add instant-distance (fast, pure Rust HNSW)
cargo add instant-distance@0.7 --features hnsw

# Optionally, add hnsw_rs (alternative HNSW implementation)
# cargo add hnsw_rs@0.10

echo "HNSW ANN dependencies added."
