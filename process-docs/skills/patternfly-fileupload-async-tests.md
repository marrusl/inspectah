---
name: patternfly-fileupload-async-tests
description: PatternFly's FileUpload hands the selected file to `onFileInputChange` on a microtask, so a `fireEvent.change` on the file input is not visible in the DOM when it returns. A synchronous assertion after it fails, and a `queryBy(...).not.toBeInTheDocument()` after it passes for the wrong reason.
---

# Testing PatternFly FileUpload

`RpmUploadModal` and `RpmBatchUploadModal` select files through
PatternFly's `FileUpload`, which wraps react-dropzone. The dropzone
resolves the `FileList` before it calls `onFileInputChange`, so the
component's state update lands on a **microtask**, not inside the
`fireEvent.change` call.

```tsx
fireEvent.change(input, { target: { files: [file] } });
// Nothing from the file is on screen yet: no filename, no validation
// HelperText, and the Upload button is still disabled.
screen.getByText(/foo\.rpm matches nginx/); // throws
```

Await the first thing the file produces:

```tsx
fireEvent.change(input, { target: { files: [file] } });
await screen.findByText(/foo\.rpm matches nginx/);
```

## Why this matters beyond one failing assertion

Two silent failure modes come out of it, and the second is the
dangerous one.

1. **`await user.click(uploadButton)` hides the problem.** Clicking
   yields to the microtask queue, so by the time the click handler runs
   the state is there and the test passes. Tests written that way pass
   while never having proved the intermediate state they look like they
   check.

2. **A negative assertion passes vacuously.** `expect(screen.queryByText(
   /matches/)).not.toBeInTheDocument()` after a bare `fireEvent.change`
   passes because the element had not rendered yet, not because the code
   removed it. A test asserting that a failed upload clears the success
   checkmark will pass against code that never clears it.

The guard: before asserting an element is **gone**, assert it is
**present** with `findBy*`. That proves the element can render in this
test's setup, so its later absence is the behaviour under test rather
than a race.

## Related

- `crates/web/ui/src/components/__tests__/RpmUploadModal.test.tsx`
