#compdef inspectah
# zsh completion for inspectah
# Place in /usr/share/zsh/site-functions/_inspectah

_inspectah_inspect() {
    _arguments -s \
        '--host-root[Root path for host inspection]:path:_files -/' \
        '-o[Write tarball to FILE]:file:_files' \
        '--output-dir[Write files to a directory]:dir:_files -/' \
        '--no-subscription[Skip bundling RHEL subscription certs]' \
        '--from-snapshot[Load snapshot from PATH]:path:_files' \
        '--inspect-only[Run inspectors only, skip renderers]' \
        '--target-version[Target bootc image version]:version:' \
        '--target-image[Full target bootc base image reference]:image:' \
        '--baseline-packages[Path to baseline package list]:file:_files' \
        '--no-baseline[Run without base image comparison]' \
        '--user-strategy[Override user creation strategy]:strategy:(sysusers blueprint useradd kickstart)' \
        '--config-diffs[Generate line-by-line diffs for modified configs]' \
        '--deep-binary-scan[Full strings scan on unknown binaries]' \
        '--query-podman[Enumerate running containers via podman]' \
        '--skip-preflight[Skip container privilege checks]' \
        '--validate[Run podman build to verify Containerfile]' \
        '--push-to-github[Push output to GitHub repository]:repo:' \
        '--github-token[GitHub personal access token]:token:' \
        '--public[Make new repo public]' \
        '--yes[Skip interactive confirmation prompts]' \
        '--sensitivity[Heuristic detection sensitivity]:sensitivity:(strict moderate)' \
        '--no-redaction[Disable all redaction — detection still runs]' \
        '--skip-unavailable[Skip the package availability preflight check]'
}

_inspectah_fleet() {
    _arguments -s \
        '1:input_dir:_files -/' \
        {-p,--min-prevalence}'[Include items present on >= PCT% of hosts]:pct:' \
        {-o,--output-file}'[Output path for tarball or JSON]:file:_files' \
        '--output-dir[Write rendered files to a directory]:dir:_files -/' \
        '--json-only[Write merged JSON only, skip rendering]' \
        '--no-hosts[Omit per-item host lists from fleet metadata]'
}

_inspectah_refine() {
    _arguments -s \
        '1:tarball:_files -g "*.tar.gz"' \
        '--no-browser[Do not auto-open the browser on startup]' \
        '--port[HTTP server port]:port:'
}

_inspectah_architect() {
    _arguments -s \
        '1:input_dir:_files -/' \
        '--port[Port for the architect web UI]:port:' \
        '--no-browser[Do not open browser automatically]' \
        '--bind[Address to bind]:address:'
}

_inspectah() {
    local -a subcmds=(
        'inspect:Inspect a host and generate migration artifacts'
        'fleet:Aggregate multiple inspection snapshots'
        'refine:Interactively edit and re-render output'
        'architect:Plan layer decomposition from refined fleets'
    )

    if (( CURRENT == 2 )); then
        if [[ "$PREFIX" == -* || "${words[CURRENT]}" == -* ]]; then
            _inspectah_inspect
        else
            _describe 'subcommand' subcmds
        fi
        return
    fi

    case "${words[2]}" in
        inspect) _inspectah_inspect ;;
        fleet)   _inspectah_fleet ;;
        refine)    _inspectah_refine ;;
        architect) _inspectah_architect ;;
        *)         _inspectah_inspect ;;
    esac
}

_inspectah "$@"
