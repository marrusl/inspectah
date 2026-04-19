# fish completion for inspectah
# Place in /usr/share/fish/vendor_completions.d/inspectah.fish

set -l subcmds scan fleet refine architect

# Subcommands
complete -c inspectah -n "not __fish_seen_subcommand_from $subcmds" -a scan -d "Scan a host and generate migration artifacts"
complete -c inspectah -n "not __fish_seen_subcommand_from $subcmds" -a fleet -d "Aggregate multiple inspection snapshots"
complete -c inspectah -n "not __fish_seen_subcommand_from $subcmds" -a refine -d "Interactively edit and re-render output"
complete -c inspectah -n "not __fish_seen_subcommand_from $subcmds" -a architect -d "Plan layer decomposition from refined fleets"

# Top-level scan flags for the backwards-compatible `inspectah --flag` form
complete -c inspectah -n "not __fish_seen_subcommand_from $subcmds" -l host-root -r -d "Root path for host inspection"
complete -c inspectah -n "not __fish_seen_subcommand_from $subcmds" -s o -r -d "Write tarball to FILE"
complete -c inspectah -n "not __fish_seen_subcommand_from $subcmds" -l output-dir -r -d "Write files to a directory"
complete -c inspectah -n "not __fish_seen_subcommand_from $subcmds" -l no-subscription -d "Skip bundling RHEL subscription certs"
complete -c inspectah -n "not __fish_seen_subcommand_from $subcmds" -l from-snapshot -r -d "Load snapshot from PATH"
complete -c inspectah -n "not __fish_seen_subcommand_from $subcmds" -l inspect-only -d "Run inspectors only, skip renderers"
complete -c inspectah -n "not __fish_seen_subcommand_from $subcmds" -l target-version -r -d "Target bootc image version"
complete -c inspectah -n "not __fish_seen_subcommand_from $subcmds" -l target-image -r -d "Full target bootc base image reference"
complete -c inspectah -n "not __fish_seen_subcommand_from $subcmds" -l baseline-packages -r -d "Path to baseline package list"
complete -c inspectah -n "not __fish_seen_subcommand_from $subcmds" -l no-baseline -d "Run without base image comparison"
complete -c inspectah -n "not __fish_seen_subcommand_from $subcmds" -l user-strategy -r -a "sysusers blueprint useradd kickstart" -d "Override user creation strategy"
complete -c inspectah -n "not __fish_seen_subcommand_from $subcmds" -l config-diffs -d "Generate line-by-line diffs for modified configs"
complete -c inspectah -n "not __fish_seen_subcommand_from $subcmds" -l deep-binary-scan -d "Full strings scan on unknown binaries"
complete -c inspectah -n "not __fish_seen_subcommand_from $subcmds" -l query-podman -d "Enumerate running containers via podman"
complete -c inspectah -n "not __fish_seen_subcommand_from $subcmds" -l skip-preflight -d "Skip container privilege checks"
complete -c inspectah -n "not __fish_seen_subcommand_from $subcmds" -l validate -d "Run podman build to verify Containerfile"
complete -c inspectah -n "not __fish_seen_subcommand_from $subcmds" -l push-to-github -r -d "Push output to GitHub repository"
complete -c inspectah -n "not __fish_seen_subcommand_from $subcmds" -l github-token -r -d "GitHub personal access token"
complete -c inspectah -n "not __fish_seen_subcommand_from $subcmds" -l public -d "Make new repo public"
complete -c inspectah -n "not __fish_seen_subcommand_from $subcmds" -l yes -d "Skip interactive confirmation prompts"
complete -c inspectah -n "not __fish_seen_subcommand_from $subcmds" -l sensitivity -r -a "strict moderate" -d "Heuristic detection sensitivity"
complete -c inspectah -n "not __fish_seen_subcommand_from $subcmds" -l no-redaction -d "Disable all redaction"
complete -c inspectah -n "not __fish_seen_subcommand_from $subcmds" -l skip-unavailable -d "Skip the package availability preflight check"

# scan flags
complete -c inspectah -n "__fish_seen_subcommand_from scan" -l host-root -r -d "Root path for host inspection"
complete -c inspectah -n "__fish_seen_subcommand_from scan" -s o -r -d "Write tarball to FILE"
complete -c inspectah -n "__fish_seen_subcommand_from scan" -l output-dir -r -d "Write files to a directory"
complete -c inspectah -n "__fish_seen_subcommand_from scan" -l no-subscription -d "Skip bundling RHEL subscription certs"
complete -c inspectah -n "__fish_seen_subcommand_from scan" -l from-snapshot -r -d "Load snapshot from PATH"
complete -c inspectah -n "__fish_seen_subcommand_from scan" -l inspect-only -d "Run inspectors only, skip renderers"
complete -c inspectah -n "__fish_seen_subcommand_from scan" -l target-version -r -d "Target bootc image version"
complete -c inspectah -n "__fish_seen_subcommand_from scan" -l target-image -r -d "Full target bootc base image reference"
complete -c inspectah -n "__fish_seen_subcommand_from scan" -l baseline-packages -r -d "Path to baseline package list"
complete -c inspectah -n "__fish_seen_subcommand_from scan" -l no-baseline -d "Run without base image comparison"
complete -c inspectah -n "__fish_seen_subcommand_from scan" -l user-strategy -r -a "sysusers blueprint useradd kickstart" -d "Override user creation strategy"
complete -c inspectah -n "__fish_seen_subcommand_from scan" -l config-diffs -d "Generate line-by-line diffs for modified configs"
complete -c inspectah -n "__fish_seen_subcommand_from scan" -l deep-binary-scan -d "Full strings scan on unknown binaries"
complete -c inspectah -n "__fish_seen_subcommand_from scan" -l query-podman -d "Enumerate running containers via podman"
complete -c inspectah -n "__fish_seen_subcommand_from scan" -l skip-preflight -d "Skip container privilege checks"
complete -c inspectah -n "__fish_seen_subcommand_from scan" -l validate -d "Run podman build to verify Containerfile"
complete -c inspectah -n "__fish_seen_subcommand_from scan" -l push-to-github -r -d "Push output to GitHub repository"
complete -c inspectah -n "__fish_seen_subcommand_from scan" -l github-token -r -d "GitHub personal access token"
complete -c inspectah -n "__fish_seen_subcommand_from scan" -l public -d "Make new repo public"
complete -c inspectah -n "__fish_seen_subcommand_from scan" -l yes -d "Skip interactive confirmation prompts"
complete -c inspectah -n "__fish_seen_subcommand_from scan" -l sensitivity -r -a "strict moderate" -d "Heuristic detection sensitivity"
complete -c inspectah -n "__fish_seen_subcommand_from scan" -l no-redaction -d "Disable all redaction"
complete -c inspectah -n "__fish_seen_subcommand_from scan" -l skip-unavailable -d "Skip the package availability preflight check"

# fleet flags
complete -c inspectah -n "__fish_seen_subcommand_from fleet" -s p -l min-prevalence -r -d "Include items present on >= PCT% of hosts"
complete -c inspectah -n "__fish_seen_subcommand_from fleet" -s o -l output-file -r -d "Output path for tarball or JSON"
complete -c inspectah -n "__fish_seen_subcommand_from fleet" -l output-dir -r -d "Write rendered files to a directory"
complete -c inspectah -n "__fish_seen_subcommand_from fleet" -l json-only -d "Write merged JSON only, skip rendering"
complete -c inspectah -n "__fish_seen_subcommand_from fleet" -l no-hosts -d "Omit per-item host lists from fleet metadata"

# refine flags
complete -c inspectah -n "__fish_seen_subcommand_from refine" -l no-browser -d "Do not auto-open the browser on startup"
complete -c inspectah -n "__fish_seen_subcommand_from refine" -l port -r -d "HTTP server port"

# architect flags
complete -c inspectah -n "__fish_seen_subcommand_from architect" -l port -r -d "Port for the architect web UI"
complete -c inspectah -n "__fish_seen_subcommand_from architect" -l no-browser -d "Do not open browser automatically"
complete -c inspectah -n "__fish_seen_subcommand_from architect" -l bind -r -d "Address to bind"
