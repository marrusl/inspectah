package cli

import "github.com/spf13/cobra"

func registerScanPassthrough(cmd *cobra.Command) {
	f := cmd.Flags()
	f.String("host-root", "", "root path for host inspection")
	f.Bool("no-subscription", false, "skip bundling RHEL subscription certs")
	f.String("from-snapshot", "", "load snapshot from path instead of inspecting")
	f.Bool("inspect-only", false, "run inspectors only, skip rendering")
	f.String("target-version", "", "target bootc image version (e.g. 9.6, 10.2)")
	f.String("target-image", "", "full target bootc base image reference")
	f.String("baseline-packages", "", "path to baseline package list for air-gapped use")
	f.Bool("no-baseline", false, "skip base image comparison")
	f.String("user-strategy", "", "override user creation strategy (sysusers, blueprint, useradd, kickstart)")
	f.Bool("config-diffs", false, "generate line-by-line diffs for modified configs")
	f.Bool("deep-binary-scan", false, "full strings scan on unknown binaries")
	f.Bool("query-podman", false, "enumerate running containers via podman socket")
	f.Bool("skip-preflight", false, "skip container privilege checks")
	f.Bool("validate", false, "run podman build to verify generated Containerfile")
	f.String("push-to-github", "", "push output to GitHub repository (owner/repo)")
	f.String("github-token", "", "GitHub personal access token")
	f.Bool("public", false, "make GitHub repo public")
	f.Bool("yes", false, "skip interactive confirmation prompts")
	f.String("sensitivity", "", "heuristic detection sensitivity (strict, moderate)")
	f.Bool("no-redaction", false, "disable all redaction")
	f.Bool("skip-unavailable", false, "skip package availability preflight check")
	f.String("output-dir", "", "write files to directory instead of tarball")

}

func registerFleetPassthrough(cmd *cobra.Command) {
	f := cmd.Flags()
	f.IntP("min-prevalence", "p", 100, "include items on >= N%% of hosts")
	f.String("output-file", "", "output tarball path")
	f.String("output-dir", "", "write to directory instead of tarball")
	f.Bool("json-only", false, "write merged JSON only")
	f.Bool("no-hosts", false, "omit per-item host lists")
}
