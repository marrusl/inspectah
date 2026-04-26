package container

import (
	"bytes"
	"context"
	"fmt"
	"io"
	"os"
	"strings"
)

func EnsureImage(ctx context.Context, runner PodmanRunner, image, pullPolicy string, w io.Writer) error {
	pinned := LoadPinnedImage()

	switch pullPolicy {
	case "never":
		if err := checkImageExists(ctx, runner, image); err != nil {
			fmt.Fprintf(w, "\nError: Migration tool container not found locally.\n")
			fmt.Fprintf(w, "  Image: %s\n", image)
			fmt.Fprintf(w, "  Fix:   Transfer the image to this host and import it:\n")
			fmt.Fprintf(w, "           podman load -i inspectah.tar\n")
			fmt.Fprintf(w, "         Or allow pulling: inspectah scan --pull=missing\n")
			return err
		}
		if pinned != "" {
			fmt.Fprintf(w, "── [setup]  Migration tool ready. (pinned: %s)\n", image)
		} else {
			fmt.Fprintf(w, "── [setup]  Migration tool ready. (cached)\n")
		}
		return nil
	case "always":
		return pullImage(ctx, runner, image, w)
	case "missing":
		if err := checkImageExists(ctx, runner, image); err != nil {
			return pullImage(ctx, runner, image, w)
		}
		if pinned != "" {
			fmt.Fprintf(w, "── [setup]  Migration tool ready. (pinned: %s)\n", image)
		} else {
			fmt.Fprintf(w, "── [setup]  Migration tool ready. (cached)\n")
		}
		return nil
	default:
		return fmt.Errorf("unknown pull policy: %s", pullPolicy)
	}
}

func checkImageExists(ctx context.Context, runner PodmanRunner, image string) error {
	var stderr bytes.Buffer
	code, err := runner.Run(ctx, []string{"image", "exists", image}, io.Discard, &stderr)
	if err != nil {
		return fmt.Errorf("failed to check image: %w", err)
	}
	if code != 0 {
		return fmt.Errorf("image not found locally: %s", image)
	}
	return nil
}

func pullImage(ctx context.Context, runner PodmanRunner, image string, w io.Writer) error {
	fmt.Fprintf(w, "── [setup]  Pulling migration tool...\n")
	fmt.Fprintf(w, "            (%s — cached after first pull)\n", image)

	pr, pw := io.Pipe()
	var stderr bytes.Buffer

	done := make(chan struct{})
	go func() {
		StreamPullProgress(pr, w)
		close(done)
	}()

	code, err := runner.Run(ctx, []string{"pull", image}, io.Discard, io.MultiWriter(pw, &stderr))
	pw.Close()
	<-done

	if err != nil {
		return fmt.Errorf("failed to pull image: %w", err)
	}
	if code != 0 {
		errMsg := strings.TrimSpace(stderr.String())
		if strings.Contains(errMsg, "manifest unknown") {
			return fmt.Errorf("image tag not found: %s — check available versions with: inspectah image info", image)
		}
		return fmt.Errorf("failed to pull image %s: %s", image, errMsg)
	}

	fmt.Fprintf(w, "   ✓  Migration tool ready.\n")
	return nil
}

func ImageVersion(ctx context.Context, runner PodmanRunner, image string) (string, error) {
	var stdout bytes.Buffer
	code, err := runner.Run(ctx, []string{"inspect", "--format", "{{.Id}}", image}, &stdout, os.Stderr)
	if err != nil || code != 0 {
		return "", fmt.Errorf("cannot inspect image %s", image)
	}
	id := strings.TrimSpace(stdout.String())
	if len(id) > 12 {
		id = id[:12]
	}
	return id, nil
}
