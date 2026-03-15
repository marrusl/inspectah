#!/usr/bin/env bash
# Build a minimal CodeMirror 6 bundle for yoinkc's config editor.
#
# Produces a single IIFE bundle exposing window.CMEditor with:
#   CMEditor.create(parent, content) → EditorView
#   CMEditor.getContent(view) → string
#   CMEditor.setContent(view, content)
#
# Requirements: node ≥18, npm
# Output: src/yoinkc/static/codemirror/codemirror.min.js
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
DEST="$REPO_ROOT/src/yoinkc/static/codemirror"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

cd "$WORK"
npm init -y --silent >/dev/null 2>&1
npm install --silent codemirror @codemirror/view @codemirror/state 2>&1 | tail -1

cat > build.mjs << 'ENTRY'
import {basicSetup} from "codemirror";
import {EditorView} from "@codemirror/view";
import {EditorState} from "@codemirror/state";

export function create(parent, content) {
  return new EditorView({
    state: EditorState.create({
      doc: content || "",
      extensions: [
        basicSetup,
        EditorView.lineWrapping,
        EditorView.theme({
          "&": {height: "100%", fontSize: "14px"},
          ".cm-scroller": {overflow: "auto"},
        }),
      ],
    }),
    parent: parent,
  });
}

export function getContent(view) {
  return view.state.doc.toString();
}

export function setContent(view, content) {
  view.dispatch({
    changes: {from: 0, to: view.state.doc.length, insert: content},
  });
}
ENTRY

npx esbuild build.mjs \
  --bundle \
  --minify \
  --format=iife \
  --global-name=CMEditor \
  --outfile=codemirror.min.js \
  2>&1 | grep -v "^$"

mkdir -p "$DEST"
cp codemirror.min.js "$DEST/codemirror.min.js"

SIZE=$(wc -c < "$DEST/codemirror.min.js" | tr -d ' ')
echo "Built $DEST/codemirror.min.js (${SIZE} bytes)"
