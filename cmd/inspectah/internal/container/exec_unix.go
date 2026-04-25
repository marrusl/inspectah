//go:build !windows

package container

import "syscall"

func execPodman(podmanPath string, args []string) error {
	argv := append([]string{podmanPath}, args...)
	return syscall.Exec(podmanPath, argv, nil)
}
