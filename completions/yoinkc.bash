# bash completion for yoinkc
# Source this file or place it in /usr/share/bash-completion/completions/yoinkc

_yoinkc() {
    local cur prev words cword
    _init_completion || return

    local subcommands="inspect fleet refine architect"

    local inspect_flags="--host-root -o --output-dir --no-subscription
        --from-snapshot --inspect-only --target-version --target-image
        --baseline-packages --no-baseline --user-strategy --config-diffs
        --deep-binary-scan --query-podman --skip-preflight --validate
        --push-to-github --github-token --public --yes
        --sensitivity --no-redaction"

    local fleet_flags="-p --min-prevalence -o --output-file --output-dir
        --json-only --no-hosts"

    local refine_flags="--no-browser --port"

    local architect_flags="--port --no-browser --bind"

    # Determine which subcommand is active
    local subcmd=""
    local i
    for (( i=1; i < cword; i++ )); do
        case "${words[i]}" in
            inspect|fleet|refine|architect)
                subcmd="${words[i]}"
                break
                ;;
        esac
    done

    if [[ -z "$subcmd" ]]; then
        if [[ "$cur" == -* ]]; then
            COMPREPLY=( $(compgen -W "$inspect_flags" -- "$cur") )
        else
            COMPREPLY=( $(compgen -W "$subcommands" -- "$cur") )
        fi
        return
    fi

    case "$subcmd" in
        inspect)
            if [[ "$cur" == -* ]]; then
                COMPREPLY=( $(compgen -W "$inspect_flags" -- "$cur") )
            elif [[ "$prev" == --host-root || "$prev" == --output-dir || \
                    "$prev" == --from-snapshot || "$prev" == --baseline-packages ]]; then
                _filedir
            elif [[ "$prev" == -o ]]; then
                _filedir
            elif [[ "$prev" == --user-strategy ]]; then
                COMPREPLY=( $(compgen -W "sysusers blueprint useradd kickstart" -- "$cur") )
            elif [[ "$prev" == --sensitivity ]]; then
                COMPREPLY=( $(compgen -W "strict moderate" -- "$cur") )
            fi
            ;;
        fleet)
            if [[ "$cur" == -* ]]; then
                COMPREPLY=( $(compgen -W "$fleet_flags" -- "$cur") )
            else
                _filedir -d
            fi
            ;;
        refine)
            if [[ "$cur" == -* ]]; then
                COMPREPLY=( $(compgen -W "$refine_flags" -- "$cur") )
            else
                _filedir '*.tar.gz'
            fi
            ;;
        architect)
            if [[ "$cur" == -* ]]; then
                COMPREPLY=( $(compgen -W "$architect_flags" -- "$cur") )
            else
                _filedir -d
            fi
            ;;
    esac
}

complete -F _yoinkc yoinkc
