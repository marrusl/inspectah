package container

import "sort"

type RunOpts struct {
	Image      string
	Privileged bool
	PIDHost    bool
	Mounts     []Mount
	Ports      []string
	Env        map[string]string
	Workdir    string
	Command    []string
}

type Mount struct {
	Source   string
	Target  string
	Options string
}

func BuildArgs(opts RunOpts) []string {
	args := []string{"run", "--rm"}

	if opts.Privileged {
		args = append(args, "--privileged")
	}
	if opts.PIDHost {
		args = append(args, "--pid=host")
	}

	args = append(args, "--security-opt", "label=disable")

	for _, m := range opts.Mounts {
		spec := m.Source + ":" + m.Target
		if m.Options != "" {
			spec += ":" + m.Options
		}
		args = append(args, "-v", spec)
	}

	for _, p := range opts.Ports {
		args = append(args, "-p", p)
	}

	envKeys := make([]string, 0, len(opts.Env))
	for k := range opts.Env {
		envKeys = append(envKeys, k)
	}
	sort.Strings(envKeys)
	for _, k := range envKeys {
		args = append(args, "-e", k+"="+opts.Env[k])
	}

	if opts.Workdir != "" {
		args = append(args, "-w", opts.Workdir)
	}

	args = append(args, opts.Image)
	args = append(args, opts.Command...)

	return args
}
