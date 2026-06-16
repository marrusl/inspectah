# Print an optspec for argparse to handle cmd's options that are independent of any subcommand.
function __fish_inspectah_global_optspecs
	string join \n markdown-help h/help V/version
end

function __fish_inspectah_needs_command
	# Figure out if the current invocation already has a command.
	set -l cmd (commandline -opc)
	set -e cmd[1]
	argparse -s (__fish_inspectah_global_optspecs) -- $cmd 2>/dev/null
	or return
	if set -q argv[1]
		# Also print the command, so this can be used to figure out what it is.
		echo $argv[1]
		return 1
	end
	return 0
end

function __fish_inspectah_using_subcommand
	set -l cmd (__fish_inspectah_needs_command)
	test -z "$cmd"
	and return 1
	contains -- $cmd[1] $argv
end

complete -c inspectah -n "__fish_inspectah_needs_command" -l markdown-help -d 'Print full CLI reference in markdown format'
complete -c inspectah -n "__fish_inspectah_needs_command" -s h -l help -d 'Print help'
complete -c inspectah -n "__fish_inspectah_needs_command" -s V -l version -d 'Print version'
complete -c inspectah -n "__fish_inspectah_needs_command" -f -a "scan" -d 'Scan the current system and produce a migration snapshot'
complete -c inspectah -n "__fish_inspectah_needs_command" -f -a "refine" -d 'Interactively refine scan output and re-render'
complete -c inspectah -n "__fish_inspectah_needs_command" -f -a "aggregate" -d 'Combine multiple host scan tarballs into an aggregate snapshot'
complete -c inspectah -n "__fish_inspectah_needs_command" -f -a "build" -d 'Build a bootc container image from an inspectah tarball snapshot'
complete -c inspectah -n "__fish_inspectah_needs_command" -f -a "version" -d 'Print version, commit, and build date'
complete -c inspectah -n "__fish_inspectah_needs_command" -f -a "completions" -d 'Generate shell completions'
complete -c inspectah -n "__fish_inspectah_needs_command" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c inspectah -n "__fish_inspectah_using_subcommand scan" -s o -l output -d 'Output file path (tarball) or directory (with --inspect-only)' -r -F
complete -c inspectah -n "__fish_inspectah_using_subcommand scan" -l base-image -d 'Target base image for cross-distro conversion (e.g., registry.redhat.io/rhel9/rhel-bootc:9.6)' -r
complete -c inspectah -n "__fish_inspectah_using_subcommand scan" -l preserve -d 'Preserve sensitive data in the snapshot' -r -f -a "password-hashes\t''
ssh-keys\t''
subscription\t''
all\t''"
complete -c inspectah -n "__fish_inspectah_using_subcommand scan" -l progress -d 'Progress display mode: pretty (default TTY), flat (non-TTY/CI)' -r -f -a "pretty\t'Arrival-order receipt with spinners and ANSI color (default for TTY)'
flat\t'Numbered sequential lines, no ANSI (CI / piped output)'"
complete -c inspectah -n "__fish_inspectah_using_subcommand scan" -l inspect-only -d 'Write JSON snapshot only, skip tarball/artifact generation'
complete -c inspectah -n "__fish_inspectah_using_subcommand scan" -l no-redaction -d 'Skip the redaction phase — secrets remain unmasked in output'
complete -c inspectah -n "__fish_inspectah_using_subcommand scan" -l ack-sensitive -l acknowledge-sensitive -d 'Acknowledge sensitive data in the snapshot (required with --preserve or --no-redaction)'
complete -c inspectah -n "__fish_inspectah_using_subcommand scan" -s v -l verbose -d 'Show sub-step detail for all inspectors, including fast ones'
complete -c inspectah -n "__fish_inspectah_using_subcommand scan" -s q -l quiet -d 'Suppress the scan progress checklist (completion summary still prints)'
complete -c inspectah -n "__fish_inspectah_using_subcommand scan" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c inspectah -n "__fish_inspectah_using_subcommand refine" -l port -d 'Port to bind (default: 8642, use 0 for ephemeral)' -r
complete -c inspectah -n "__fish_inspectah_using_subcommand refine" -l open -d 'Open browser automatically (use --no-open to suppress)' -r -f -a "true\t''
false\t''"
complete -c inspectah -n "__fish_inspectah_using_subcommand refine" -l fresh -d 'Start a fresh session, discarding any saved progress'
complete -c inspectah -n "__fish_inspectah_using_subcommand refine" -l tui -d 'Use terminal UI instead of web browser'
complete -c inspectah -n "__fish_inspectah_using_subcommand refine" -s h -l help -d 'Print help'
complete -c inspectah -n "__fish_inspectah_using_subcommand aggregate; and not __fish_seen_subcommand_from init help" -l manifest -d 'Path to an aggregate manifest (TOML) specifying sources' -r -F
complete -c inspectah -n "__fish_inspectah_using_subcommand aggregate; and not __fish_seen_subcommand_from init help" -l target-image -d 'Override the target image reference for baseline comparison' -r
complete -c inspectah -n "__fish_inspectah_using_subcommand aggregate; and not __fish_seen_subcommand_from init help" -l output-dir -d 'Output directory for the aggregate tarball' -r -F
complete -c inspectah -n "__fish_inspectah_using_subcommand aggregate; and not __fish_seen_subcommand_from init help" -l output-file -d 'Output file path for the aggregate tarball' -r -F
complete -c inspectah -n "__fish_inspectah_using_subcommand aggregate; and not __fish_seen_subcommand_from init help" -l json-only -d 'Write JSON snapshot instead of tarball (to stdout, --output-file, or --output-dir)'
complete -c inspectah -n "__fish_inspectah_using_subcommand aggregate; and not __fish_seen_subcommand_from init help" -l strict -d 'Treat warnings as errors'
complete -c inspectah -n "__fish_inspectah_using_subcommand aggregate; and not __fish_seen_subcommand_from init help" -s v -l verbose -d 'Show per-host detail in output'
complete -c inspectah -n "__fish_inspectah_using_subcommand aggregate; and not __fish_seen_subcommand_from init help" -l ack-sensitive -l acknowledge-sensitive -d 'Acknowledge that the merged output may contain sensitive data (subscription certs, password hashes, SSH keys)'
complete -c inspectah -n "__fish_inspectah_using_subcommand aggregate; and not __fish_seen_subcommand_from init help" -s h -l help -d 'Print help'
complete -c inspectah -n "__fish_inspectah_using_subcommand aggregate; and not __fish_seen_subcommand_from init help" -a "init" -d 'Generate an aggregate manifest from a directory of tarballs'
complete -c inspectah -n "__fish_inspectah_using_subcommand aggregate; and not __fish_seen_subcommand_from init help" -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c inspectah -n "__fish_inspectah_using_subcommand aggregate; and __fish_seen_subcommand_from init" -l output -d 'Output path for the generated manifest' -r -F
complete -c inspectah -n "__fish_inspectah_using_subcommand aggregate; and __fish_seen_subcommand_from init" -l overwrite -d 'Overwrite an existing manifest file'
complete -c inspectah -n "__fish_inspectah_using_subcommand aggregate; and __fish_seen_subcommand_from init" -s h -l help -d 'Print help'
complete -c inspectah -n "__fish_inspectah_using_subcommand aggregate; and __fish_seen_subcommand_from help" -f -a "init" -d 'Generate an aggregate manifest from a directory of tarballs'
complete -c inspectah -n "__fish_inspectah_using_subcommand aggregate; and __fish_seen_subcommand_from help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c inspectah -n "__fish_inspectah_using_subcommand build" -s t -l tag -d 'Image tag (must include version, e.g., \'myimage:v1\')' -r
complete -c inspectah -n "__fish_inspectah_using_subcommand build" -l dry-run -d 'Show the build command without executing it'
complete -c inspectah -n "__fish_inspectah_using_subcommand build" -l keep-context -d 'Keep the extracted build context after build completes'
complete -c inspectah -n "__fish_inspectah_using_subcommand build" -s h -l help -d 'Print help'
complete -c inspectah -n "__fish_inspectah_using_subcommand version" -s h -l help -d 'Print help'
complete -c inspectah -n "__fish_inspectah_using_subcommand completions" -s h -l help -d 'Print help'
complete -c inspectah -n "__fish_inspectah_using_subcommand help; and not __fish_seen_subcommand_from scan refine aggregate build version completions help" -f -a "scan" -d 'Scan the current system and produce a migration snapshot'
complete -c inspectah -n "__fish_inspectah_using_subcommand help; and not __fish_seen_subcommand_from scan refine aggregate build version completions help" -f -a "refine" -d 'Interactively refine scan output and re-render'
complete -c inspectah -n "__fish_inspectah_using_subcommand help; and not __fish_seen_subcommand_from scan refine aggregate build version completions help" -f -a "aggregate" -d 'Combine multiple host scan tarballs into an aggregate snapshot'
complete -c inspectah -n "__fish_inspectah_using_subcommand help; and not __fish_seen_subcommand_from scan refine aggregate build version completions help" -f -a "build" -d 'Build a bootc container image from an inspectah tarball snapshot'
complete -c inspectah -n "__fish_inspectah_using_subcommand help; and not __fish_seen_subcommand_from scan refine aggregate build version completions help" -f -a "version" -d 'Print version, commit, and build date'
complete -c inspectah -n "__fish_inspectah_using_subcommand help; and not __fish_seen_subcommand_from scan refine aggregate build version completions help" -f -a "completions" -d 'Generate shell completions'
complete -c inspectah -n "__fish_inspectah_using_subcommand help; and not __fish_seen_subcommand_from scan refine aggregate build version completions help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c inspectah -n "__fish_inspectah_using_subcommand help; and __fish_seen_subcommand_from aggregate" -f -a "init" -d 'Generate an aggregate manifest from a directory of tarballs'
