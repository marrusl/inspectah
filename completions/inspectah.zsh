#compdef inspectah

autoload -U is-at-least

_inspectah() {
    typeset -A opt_args
    typeset -a _arguments_options
    local ret=1

    if is-at-least 5.2; then
        _arguments_options=(-s -S -C)
    else
        _arguments_options=(-s -C)
    fi

    local context curcontext="$curcontext" state line
    _arguments "${_arguments_options[@]}" : \
'--markdown-help[Print full CLI reference in markdown format]' \
'-h[Print help]' \
'--help[Print help]' \
'-V[Print version]' \
'--version[Print version]' \
":: :_inspectah_commands" \
"*::: :->inspectah" \
&& ret=0
    case $state in
    (inspectah)
        words=($line[1] "${words[@]}")
        (( CURRENT += 1 ))
        curcontext="${curcontext%:*:*}:inspectah-command-$line[1]:"
        case $line[1] in
            (scan)
_arguments "${_arguments_options[@]}" : \
'-o+[Output file path (tarball) or directory (with --inspect-only)]:OUTPUT:_files' \
'--output=[Output file path (tarball) or directory (with --inspect-only)]:OUTPUT:_files' \
'--base-image=[Target base image for cross-distro conversion (e.g., registry.redhat.io/rhel9/rhel-bootc\:9.6)]:BASE_IMAGE:_default' \
'*--preserve=[Preserve sensitive data in the snapshot]:ITEM:(password-hashes ssh-keys subscription all)' \
'--progress=[Progress display mode\: pretty (default TTY), flat (non-TTY/CI)]:MODE:((pretty\:"Arrival-order receipt with spinners and ANSI color (default for TTY)"
flat\:"Numbered sequential lines, no ANSI (CI / piped output)"))' \
'--inspect-only[Write JSON snapshot only, skip tarball/artifact generation]' \
'--no-redaction[Skip the redaction phase — secrets remain unmasked in output]' \
'--ack-sensitive[Acknowledge sensitive data in the snapshot (required with --preserve or --no-redaction)]' \
'--acknowledge-sensitive[Acknowledge sensitive data in the snapshot (required with --preserve or --no-redaction)]' \
'(-q --quiet)-v[Show sub-step detail for all inspectors, including fast ones]' \
'(-q --quiet)--verbose[Show sub-step detail for all inspectors, including fast ones]' \
'(-v --verbose)-q[Suppress the scan progress checklist (completion summary still prints)]' \
'(-v --verbose)--quiet[Suppress the scan progress checklist (completion summary still prints)]' \
'-h[Print help (see more with '\''--help'\'')]' \
'--help[Print help (see more with '\''--help'\'')]' \
&& ret=0
;;
(refine)
_arguments "${_arguments_options[@]}" : \
'--port=[Port to bind (default\: 8642, use 0 for ephemeral)]:PORT:_default' \
'--open=[Open browser automatically (use --no-open to suppress)]:OPEN:(true false)' \
'--fresh[Start a fresh session, discarding any saved progress]' \
'--tui[Use terminal UI instead of web browser]' \
'-h[Print help]' \
'--help[Print help]' \
':tarball -- Path to scan output tarball (.tar.gz):_files' \
&& ret=0
;;
(aggregate)
_arguments "${_arguments_options[@]}" : \
'--target-image=[Override the target image reference for baseline comparison]:TARGET_IMAGE:_default' \
'--output-dir=[Output directory for the aggregate tarball]:OUTPUT_DIR:_files' \
'--output-file=[Output file path for the aggregate tarball]:OUTPUT_FILE:_files' \
'--json-only[Write JSON snapshot instead of tarball (to stdout, --output-file, or --output-dir)]' \
'--strict[Treat warnings as errors]' \
'-v[Show per-host detail in output]' \
'--verbose[Show per-host detail in output]' \
'--ack-sensitive[Acknowledge that the merged output may contain sensitive data (subscription certs, password hashes, SSH keys)]' \
'--acknowledge-sensitive[Acknowledge that the merged output may contain sensitive data (subscription certs, password hashes, SSH keys)]' \
'-h[Print help]' \
'--help[Print help]' \
'*::inputs -- Input tarballs or directory containing tarballs:_files' \
&& ret=0
;;
(build)
_arguments "${_arguments_options[@]}" : \
'-t+[Image tag (must include version, e.g., '\''myimage\:v1'\'')]:TAG:_default' \
'--tag=[Image tag (must include version, e.g., '\''myimage\:v1'\'')]:TAG:_default' \
'--dry-run[Show the build command without executing it]' \
'--keep-context[Keep the extracted build context after build completes]' \
'-h[Print help]' \
'--help[Print help]' \
':tarball -- Path to inspectah tarball (.tar.gz snapshot):_files' \
'*::podman_args -- Additional arguments to pass to podman build (after --):_default' \
&& ret=0
;;
(version)
_arguments "${_arguments_options[@]}" : \
'-h[Print help]' \
'--help[Print help]' \
&& ret=0
;;
(completions)
_arguments "${_arguments_options[@]}" : \
'-h[Print help]' \
'--help[Print help]' \
':shell -- Shell to generate for:(bash elvish fish powershell zsh)' \
&& ret=0
;;
(help)
_arguments "${_arguments_options[@]}" : \
":: :_inspectah__subcmd__help_commands" \
"*::: :->help" \
&& ret=0

    case $state in
    (help)
        words=($line[1] "${words[@]}")
        (( CURRENT += 1 ))
        curcontext="${curcontext%:*:*}:inspectah-help-command-$line[1]:"
        case $line[1] in
            (scan)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(refine)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(aggregate)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(build)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(version)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(completions)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(help)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
        esac
    ;;
esac
;;
        esac
    ;;
esac
}

(( $+functions[_inspectah_commands] )) ||
_inspectah_commands() {
    local commands; commands=(
'scan:Scan the current system and produce a migration snapshot' \
'refine:Interactively refine scan output and re-render' \
'aggregate:Combine multiple host scan tarballs into an aggregate snapshot' \
'build:Build a bootc container image from an inspectah tarball snapshot' \
'version:Print version, commit, and build date' \
'completions:Generate shell completions' \
'help:Print this message or the help of the given subcommand(s)' \
    )
    _describe -t commands 'inspectah commands' commands "$@"
}
(( $+functions[_inspectah__subcmd__aggregate_commands] )) ||
_inspectah__subcmd__aggregate_commands() {
    local commands; commands=()
    _describe -t commands 'inspectah aggregate commands' commands "$@"
}
(( $+functions[_inspectah__subcmd__build_commands] )) ||
_inspectah__subcmd__build_commands() {
    local commands; commands=()
    _describe -t commands 'inspectah build commands' commands "$@"
}
(( $+functions[_inspectah__subcmd__completions_commands] )) ||
_inspectah__subcmd__completions_commands() {
    local commands; commands=()
    _describe -t commands 'inspectah completions commands' commands "$@"
}
(( $+functions[_inspectah__subcmd__help_commands] )) ||
_inspectah__subcmd__help_commands() {
    local commands; commands=(
'scan:Scan the current system and produce a migration snapshot' \
'refine:Interactively refine scan output and re-render' \
'aggregate:Combine multiple host scan tarballs into an aggregate snapshot' \
'build:Build a bootc container image from an inspectah tarball snapshot' \
'version:Print version, commit, and build date' \
'completions:Generate shell completions' \
'help:Print this message or the help of the given subcommand(s)' \
    )
    _describe -t commands 'inspectah help commands' commands "$@"
}
(( $+functions[_inspectah__subcmd__help__subcmd__aggregate_commands] )) ||
_inspectah__subcmd__help__subcmd__aggregate_commands() {
    local commands; commands=()
    _describe -t commands 'inspectah help aggregate commands' commands "$@"
}
(( $+functions[_inspectah__subcmd__help__subcmd__build_commands] )) ||
_inspectah__subcmd__help__subcmd__build_commands() {
    local commands; commands=()
    _describe -t commands 'inspectah help build commands' commands "$@"
}
(( $+functions[_inspectah__subcmd__help__subcmd__completions_commands] )) ||
_inspectah__subcmd__help__subcmd__completions_commands() {
    local commands; commands=()
    _describe -t commands 'inspectah help completions commands' commands "$@"
}
(( $+functions[_inspectah__subcmd__help__subcmd__help_commands] )) ||
_inspectah__subcmd__help__subcmd__help_commands() {
    local commands; commands=()
    _describe -t commands 'inspectah help help commands' commands "$@"
}
(( $+functions[_inspectah__subcmd__help__subcmd__refine_commands] )) ||
_inspectah__subcmd__help__subcmd__refine_commands() {
    local commands; commands=()
    _describe -t commands 'inspectah help refine commands' commands "$@"
}
(( $+functions[_inspectah__subcmd__help__subcmd__scan_commands] )) ||
_inspectah__subcmd__help__subcmd__scan_commands() {
    local commands; commands=()
    _describe -t commands 'inspectah help scan commands' commands "$@"
}
(( $+functions[_inspectah__subcmd__help__subcmd__version_commands] )) ||
_inspectah__subcmd__help__subcmd__version_commands() {
    local commands; commands=()
    _describe -t commands 'inspectah help version commands' commands "$@"
}
(( $+functions[_inspectah__subcmd__refine_commands] )) ||
_inspectah__subcmd__refine_commands() {
    local commands; commands=()
    _describe -t commands 'inspectah refine commands' commands "$@"
}
(( $+functions[_inspectah__subcmd__scan_commands] )) ||
_inspectah__subcmd__scan_commands() {
    local commands; commands=()
    _describe -t commands 'inspectah scan commands' commands "$@"
}
(( $+functions[_inspectah__subcmd__version_commands] )) ||
_inspectah__subcmd__version_commands() {
    local commands; commands=()
    _describe -t commands 'inspectah version commands' commands "$@"
}

if [ "$funcstack[1]" = "_inspectah" ]; then
    _inspectah "$@"
else
    compdef _inspectah inspectah
fi
