package container

import (
	"bufio"
	"fmt"
	"io"
	"strings"
)

func StreamPullProgress(r io.Reader, w io.Writer) {
	scanner := bufio.NewScanner(r)
	for scanner.Scan() {
		line := scanner.Text()
		if strings.Contains(line, "Copying blob") || strings.Contains(line, "Copying config") {
			fmt.Fprintf(w, "  %s\n", line)
		} else if strings.Contains(line, "Writing manifest") {
			fmt.Fprintf(w, "  %s\n", line)
		} else if strings.Contains(line, "Storing signatures") {
			fmt.Fprintf(w, "  %s\n", line)
		}
	}
}
