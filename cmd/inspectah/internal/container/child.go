package container

import (
	"context"
	"fmt"
	"net/http"
	"os"
	"os/exec"
	"os/signal"
	"syscall"
	"time"
)

func RunChild(ctx context.Context, podmanPath string, args []string) error {
	cmd := exec.CommandContext(ctx, podmanPath, args...)
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	cmd.Stdin = os.Stdin

	sigCh := make(chan os.Signal, 1)
	signal.Notify(sigCh, syscall.SIGINT, syscall.SIGTERM)

	if err := cmd.Start(); err != nil {
		return fmt.Errorf("failed to start podman: %w", err)
	}

	go func() {
		sig := <-sigCh
		if cmd.Process != nil {
			cmd.Process.Signal(sig)
		}
	}()

	return cmd.Wait()
}

func WaitForServer(url string, timeout time.Duration) bool {
	deadline := time.Now().Add(timeout)
	client := &http.Client{Timeout: 500 * time.Millisecond}
	for time.Now().Before(deadline) {
		resp, err := client.Get(url)
		if err == nil {
			resp.Body.Close()
			if resp.StatusCode == 200 {
				return true
			}
		}
		time.Sleep(500 * time.Millisecond)
	}
	return false
}

func OpenBrowser(url string) {
	var cmd *exec.Cmd
	switch {
	case commandExists("open"):
		cmd = exec.Command("open", url)
	case commandExists("xdg-open"):
		cmd = exec.Command("xdg-open", url)
	default:
		fmt.Fprintf(os.Stderr, "  Open %s in your browser\n", url)
		return
	}
	cmd.Start()
}

func commandExists(name string) bool {
	_, err := exec.LookPath(name)
	return err == nil
}
