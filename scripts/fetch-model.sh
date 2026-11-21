#!/usr/bin/env bash
# Download all-MiniLM-L6-v2 in ONNX form, and the vocabulary it tokenises with.
#
# The model is 86 MB and is deliberately not in the repository. Everything in
# DayDoc works without it; with it, `daydoc link` and `daydoc embed` compare
# meaning rather than vocabulary.
set -euo pipefail

DEST="${1:-${DAYDOC_HOME:-$HOME/.daydoc}/models}"
BASE="https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main"

MODEL_SHA="6fd5d72fe4589f189f8ebc006442dbb529bb7ce38f8082112682524616046452"
VOCAB_SHA="07eced375cec144d27c900241f3e339478dec958f92fddbc551f295c992038a3"

mkdir -p "$DEST"

fetch() {
  local url="$1" out="$2" sha="$3"
  if [ -f "$out" ] && [ "$(sha256sum "$out" | cut -d' ' -f1)" = "$sha" ]; then
    echo "already there: $out"
    return
  fi
  echo "downloading $url"
  curl -fL --retry 3 --progress-bar -o "$out.part" "$url"
  mv "$out.part" "$out"
  local got
  got="$(sha256sum "$out" | cut -d' ' -f1)"
  if [ "$got" != "$sha" ]; then
    echo "checksum mismatch for $out" >&2
    echo "  expected $sha" >&2
    echo "  got      $got" >&2
    exit 1
  fi
  echo "ok: $out"
}

fetch "$BASE/onnx/model.onnx" "$DEST/model.onnx" "$MODEL_SHA"
fetch "$BASE/vocab.txt"        "$DEST/vocab.txt" "$VOCAB_SHA"

echo
echo "model in $DEST"
echo "check it with: daydoc embed"
