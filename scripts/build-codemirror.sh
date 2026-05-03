#!/usr/bin/env bash
# Build a CodeMirror 6 bundle for inspectah's config editor.
#
# Produces a single IIFE bundle exposing window.CM with:
#   CM.EditorState, CM.EditorView, CM.basicSetup, CM.jsonLang, CM.keymap, CM.defaultKeymap, CM.Prec
#   CM.createJSONViewer(parent, content) → EditorView
#   CM.createEditor(parent, content, opts) → EditorView
#     opts.language: language extension
#     opts.onChange: callback(newContent)
#     opts.extensions: array of custom extensions to append
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
npm install --silent codemirror @codemirror/view @codemirror/state @codemirror/language @codemirror/lang-json @codemirror/commands 2>&1 | tail -1

cat > build.mjs << 'ENTRY'
import {basicSetup} from "codemirror";
import {EditorView, keymap} from "@codemirror/view";
import {EditorState, Prec} from "@codemirror/state";
import {LanguageSupport} from "@codemirror/language";
import {json} from "@codemirror/lang-json";
import {defaultKeymap} from "@codemirror/commands";

function jsonLang() {
  return new LanguageSupport(json().language);
}

function createJSONViewer(parent, content) {
  return new EditorView({
    state: EditorState.create({
      doc: typeof content === "string" ? content : JSON.stringify(content, null, 2),
      extensions: [
        basicSetup,
        jsonLang(),
        EditorState.readOnly.of(true),
        EditorView.theme({
          "&": {fontSize: "13px"},
          ".cm-gutters": {backgroundColor: "transparent", border: "none"},
          ".cm-content": {fontFamily: "'SF Mono', 'Fira Code', monospace"}
        })
      ]
    }),
    parent: parent
  });
}

function createEditor(parent, content, opts = {}) {
  var extensions = [basicSetup];

  if (opts.language) {
    extensions.push(opts.language);
  }

  if (opts.onChange) {
    extensions.push(EditorView.updateListener.of(function(update) {
      if (update.docChanged && opts.onChange) {
        opts.onChange(update.state.doc.toString());
      }
    }));
  }

  extensions.push(EditorView.theme({
    "&": {fontSize: "13px"},
    ".cm-gutters": {backgroundColor: "transparent", border: "none"},
    ".cm-content": {fontFamily: "'SF Mono', 'Fira Code', monospace"}
  }));

  // Append custom extensions if provided
  if (opts.extensions) {
    for (var i = 0; i < opts.extensions.length; i++) {
      extensions.push(opts.extensions[i]);
    }
  }

  return new EditorView({
    state: EditorState.create({
      doc: content || "",
      extensions: extensions
    }),
    parent: parent
  });
}

export {
  EditorState,
  EditorView,
  basicSetup,
  jsonLang,
  keymap,
  defaultKeymap,
  Prec,
  createJSONViewer,
  createEditor
};
ENTRY

npx esbuild build.mjs \
  --bundle \
  --minify \
  --format=iife \
  --global-name=CM \
  --outfile=codemirror.min.js \
  2>&1 | grep -v "^$"

mkdir -p "$DEST"
cp codemirror.min.js "$DEST/codemirror.min.js"

SIZE=$(wc -c < "$DEST/codemirror.min.js" | tr -d ' ')
echo "Built $DEST/codemirror.min.js (${SIZE} bytes)"
