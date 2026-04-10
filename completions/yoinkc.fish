# fish completion for yoinkc
# Place in /usr/share/fish/vendor_completions.d/yoinkc.fish

set -l subcmds inspect fleet refine architect

# Subcommands
complete -c yoinkc -n "not __fish_seen_subcommand_from $subcmds" -a inspect -d "Inspect a host and generate migration artifacts"
complete -c yoinkc -n "not __fish_seen_subcommand_from $subcmds" -a fleet -d "Aggregate multiple inspection snapshots"
complete -c yoinkc -n "not __fish_seen_subcommand_from $subcmds" -a refine -d "Interactively edit and re-render output"
complete -c yoinkc -n "not __fish_seen_subcommand_from $subcmds" -a architect -d "Plan layer decomposition from refined fleets"

# Top-level inspect flags for the backwards-compatible `yoinkc --flag` form
complete -c yoinkc -n "not __fish_seen_subcommand_from $subcmds" -l host-root -r -d "Root path for host inspection"
complete -c yoinkc -n "not __fish_seen_subcommand_from $subcmds" -s o -r -d "Write tarball to FILE"
complete -c yoinkc -n "not __fish_seen_subcommand_from $subcmds" -l output-dir -r -d "Write files to a directory"
complete -c yoinkc -n "not __fish_seen_subcommand_from $subcmds" -l no-subscription -d "Skip bundling RHEL subscription certs"
complete -c yoinkc -n "not __fish_seen_subcommand_from $subcmds" -l from-snapshot -r -d "Load snapshot from PATH"
complete -c yoinkc -n "not __fish_seen_subcommand_from $subcmds" -l inspect-only -d "Run inspectors only, skip renderers"
complete -c yoinkc -n "not __fish_seen_subcommand_from $subcmds" -l target-version -r -d "Target bootc image version"
complete -c yoinkc -n "not __fish_seen_subcommand_from $subcmds" -l target-image -r -d "Full target bootc base image reference"
complete -c yoinkc -n "not __fish_seen_subcommand_from $subcmds" -l baseline-packages -r -d "Path to baseline package list"
complete -c yoinkc -n "not __fish_seen_subcommand_from $subcmds" -l no-baseline -d "Run without base image comparison"
complete -c yoinkc -n "not __fish_seen_subcommand_from $subcmds" -l user-strategy -r -a "sysusers blueprint useradd kickstart" -d "Override user creation strategy"
complete -c yoinkc -n "not __fish_seen_subcommand_from $subcmds" -l config-diffs -d "Generate line-by-line diffs for modified configs"
complete -c yoinkc -n "not __fish_seen_subcommand_from $subcmds" -l deep-binary-scan -d "Full strings scan on unknown binaries"
complete -c yoinkc -n "not __fish_seen_subcommand_from $subcmds" -l query-podman -d "Enumerate running containers via podman"
complete -c yoinkc -n "not __fish_seen_subcommand_from $subcmds" -l skip-preflight -d "Skip container privilege checks"
complete -c yoinkc -n "not __fish_seen_subcommand_from $subcmds" -l validate -d "Run podman build to verify Containerfile"
complete -c yoinkc -n "not __fish_seen_subcommand_from $subcmds" -l push-to-github -r -d "Push output to GitHub repository"
complete -c yoinkc -n "not __fish_seen_subcommand_from $subcmds" -l github-token -r -d "GitHub personal access token"
complete -c yoinkc -n "not __fish_seen_subcommand_from $subcmds" -l public -d "Make new repo public"
complete -c yoinkc -n "not __fish_seen_subcommand_from $subcmds" -l yes -d "Skip interactive confirmation prompts"
complete -c yoinkc -n "not __fish_seen_subcommand_from $subcmds" -l sensitivity -r -a "strict moderate" -d "Heuristic detection sensitivity"
complete -c yoinkc -n "not __fish_seen_subcommand_from $subcmds" -l no-redaction -d "Disable all redaction"
complete -c yoinkc -n "not __fish_seen_subcommand_from $subcmds" -l skip-unavailable -d "Skip the package availability preflight check"

# inspect flags
complete -c yoinkc -n "__fish_seen_subcommand_from inspect" -l host-root -r -d "Root path for host inspection"
complete -c yoinkc -n "__fish_seen_subcommand_from inspect" -s o -r -d "Write tarball to FILE"
complete -c yoinkc -n "__fish_seen_subcommand_from inspect" -l output-dir -r -d "Write files to a directory"
complete -c yoinkc -n "__fish_seen_subcommand_from inspect" -l no-subscription -d "Skip bundling RHEL subscription certs"
complete -c yoinkc -n "__fish_seen_subcommand_from inspect" -l from-snapshot -r -d "Load snapshot from PATH"
complete -c yoinkc -n "__fish_seen_subcommand_from inspect" -l inspect-only -d "Run inspectors only, skip renderers"
complete -c yoinkc -n "__fish_seen_subcommand_from inspect" -l target-version -r -d "Target bootc image version"
complete -c yoinkc -n "__fish_seen_subcommand_from inspect" -l target-image -r -d "Full target bootc base image reference"
complete -c yoinkc -n "__fish_seen_subcommand_from inspect" -l baseline-packages -r -d "Path to baseline package list"
complete -c yoinkc -n "__fish_seen_subcommand_from inspect" -l no-baseline -d "Run without base image comparison"
complete -c yoinkc -n "__fish_seen_subcommand_from inspect" -l user-strategy -r -a "sysusers blueprint useradd kickstart" -d "Override user creation strategy"
complete -c yoinkc -n "__fish_seen_subcommand_from inspect" -l config-diffs -d "Generate line-by-line diffs for modified configs"
complete -c yoinkc -n "__fish_seen_subcommand_from inspect" -l deep-binary-scan -d "Full strings scan on unknown binaries"
complete -c yoinkc -n "__fish_seen_subcommand_from inspect" -l query-podman -d "Enumerate running containers via podman"
complete -c yoinkc -n "__fish_seen_subcommand_from inspect" -l skip-preflight -d "Skip container privilege checks"
complete -c yoinkc -n "__fish_seen_subcommand_from inspect" -l validate -d "Run podman build to verify Containerfile"
complete -c yoinkc -n "__fish_seen_subcommand_from inspect" -l push-to-github -r -d "Push output to GitHub repository"
complete -c yoinkc -n "__fish_seen_subcommand_from inspect" -l github-token -r -d "GitHub personal access token"
complete -c yoinkc -n "__fish_seen_subcommand_from inspect" -l public -d "Make new repo public"
complete -c yoinkc -n "__fish_seen_subcommand_from inspect" -l yes -d "Skip interactive confirmation prompts"
complete -c yoinkc -n "__fish_seen_subcommand_from inspect" -l sensitivity -r -a "strict moderate" -d "Heuristic detection sensitivity"
complete -c yoinkc -n "__fish_seen_subcommand_from inspect" -l no-redaction -d "Disable all redaction"
complete -c yoinkc -n "__fish_seen_subcommand_from inspect" -l skip-unavailable -d "Skip the package availability preflight check"

# fleet flags
complete -c yoinkc -n "__fish_seen_subcommand_from fleet" -s p -l min-prevalence -r -d "Include items present on >= PCT% of hosts"
complete -c yoinkc -n "__fish_seen_subcommand_from fleet" -s o -l output-file -r -d "Output path for tarball or JSON"
complete -c yoinkc -n "__fish_seen_subcommand_from fleet" -l output-dir -r -d "Write rendered files to a directory"
complete -c yoinkc -n "__fish_seen_subcommand_from fleet" -l json-only -d "Write merged JSON only, skip rendering"
complete -c yoinkc -n "__fish_seen_subcommand_from fleet" -l no-hosts -d "Omit per-item host lists from fleet metadata"

# refine flags
complete -c yoinkc -n "__fish_seen_subcommand_from refine" -l no-browser -d "Do not auto-open the browser on startup"
complete -c yoinkc -n "__fish_seen_subcommand_from refine" -l port -r -d "HTTP server port"

# architect flags
complete -c yoinkc -n "__fish_seen_subcommand_from architect" -l port -r -d "Port for the architect web UI"
complete -c yoinkc -n "__fish_seen_subcommand_from architect" -l no-browser -d "Do not open browser automatically"
complete -c yoinkc -n "__fish_seen_subcommand_from architect" -l bind -r -d "Address to bind"
