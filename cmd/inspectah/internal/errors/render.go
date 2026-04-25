package errors

import (
	"fmt"
	"io"
)

func Render(w io.Writer, err *WrapperError) {
	fmt.Fprintf(w, "Error: %s\n", err.Message)
	if err.Hint != "" {
		fmt.Fprintf(w, "  Hint: %s\n", err.Hint)
	}
}
