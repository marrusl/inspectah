#!/usr/bin/env bash
# Build a CodeMirror 6 bundle for inspectah's config editor.
#
# Produces a single IIFE bundle exposing window.CMEditor with:
#   CMEditor.create(parent, content, onChange) → EditorView
#   CMEditor.getContent(view) → string
#   CMEditor.setContent(view, content)
#   CMEditor.enableVim(view)
#   CMEditor.disableVim(view)
#
# Requirements: node ≥18, npm
# Output: src/inspectah/static/codemirror/codemirror.min.js
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
DEST="$REPO_ROOT/src/inspectah/static/codemirror"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

cd "$WORK"
npm init -y --silent >/dev/null 2>&1
npm install --silent codemirror @codemirror/view @codemirror/state @replit/codemirror-vim 2>&1 | tail -1

cat > build.mjs << 'ENTRY'
import {basicSetup} from "codemirror";
import {EditorView} from "@codemirror/view";
import {EditorState, Compartment} from "@codemirror/state";
import {vim} from "@replit/codemirror-vim";

var vimCompartment = new Compartment();

export function create(parent, content, onChange) {
  var extensions = [
    basicSetup,
    EditorView.lineWrapping,
    EditorView.theme({
      "&": {height: "100%", fontSize: "14px"},
      ".cm-scroller": {overflow: "auto"},
    }),
    vimCompartment.of([]),
  ];
  if (typeof onChange === "function") {
    extensions.push(EditorView.updateListener.of(function(update) {
      if (update.docChanged) onChange(update.state.doc.toString());
    }));
  }
  return new EditorView({
    state: EditorState.create({
      doc: content || "",
      extensions: extensions,
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

export function enableVim(view) {
  view.dispatch({effects: vimCompartment.reconfigure(vim())});
}

export function disableVim(view) {
  view.dispatch({effects: vimCompartment.reconfigure([])});
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
