package renderer

import _ "embed"

//go:embed static/report.html
var reportTemplate string

//go:embed static/patternfly.min.css
var patternFlyCSS string

//go:embed static/codemirror.min.js
var codeMirrorJS string
