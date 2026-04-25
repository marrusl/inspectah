package errors

import "fmt"

type ErrorKind int

const (
	ErrUnknown ErrorKind = iota
	ErrPodmanNotFound
	ErrPodmanVersion
	ErrImageNotFound
	ErrImagePullFailed
	ErrPermissionDenied
	ErrBindMountFailed
	ErrContainerFailed
	ErrPlatformUnsupported
	ErrBuildFailed
	ErrOutputPathInvalid
	ErrConfigInvalid
)

type WrapperError struct {
	Kind    ErrorKind
	Message string
	Hint    string
	Cause   error
}

func (e *WrapperError) Error() string {
	if e.Cause != nil {
		return fmt.Sprintf("%s: %v", e.Message, e.Cause)
	}
	return e.Message
}

func (e *WrapperError) Unwrap() error {
	return e.Cause
}

func New(kind ErrorKind, message, hint string, cause error) *WrapperError {
	return &WrapperError{
		Kind:    kind,
		Message: message,
		Hint:    hint,
		Cause:   cause,
	}
}
