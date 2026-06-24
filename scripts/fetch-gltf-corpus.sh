#!/usr/bin/env bash
# Fetch a corpus of real-world glTF/GLB models for the ignored compatibility sweep.
#
# Pulls the Khronos glTF-Sample-Assets repository (the canonical conformance corpus) into
# models/corpus/. The repo is large; this clones with --depth 1 to keep it lean.
#
# Usage:
#   ./scripts/fetch-gltf-corpus.sh [target-dir]
#   GLTF_CORPUS_DIR=models/corpus cargo test --test gltf_corpus \
#       --features "gltf textures" -- --ignored --nocapture
set -euo pipefail

TARGET="${1:-models/corpus}"
REPO="https://github.com/KhronosGroup/glTF-Sample-Assets.git"

if [ -d "$TARGET/.git" ]; then
  echo "Updating existing corpus in $TARGET"
  git -C "$TARGET" pull --ff-only
else
  echo "Cloning glTF sample assets into $TARGET (shallow)"
  git clone --depth 1 "$REPO" "$TARGET"
fi

count=$(find "$TARGET" \( -name '*.gltf' -o -name '*.glb' \) | wc -l | tr -d ' ')
echo "Corpus ready: $count model file(s) under $TARGET"
echo "Run: GLTF_CORPUS_DIR=$TARGET cargo test --test gltf_corpus --features \"gltf textures\" -- --ignored --nocapture"
