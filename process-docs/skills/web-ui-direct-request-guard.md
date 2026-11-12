---
name: web-ui-direct-request-guard
description: A refine UI control that sends its request directly instead of through App's `useMutation` has no `isPending`, so it must go through `usePendingGuard`, keyed by the item it changes, or a double click or a repeated key press sends the request twice.
---

# Direct-request controls need `usePendingGuard`

App's `useMutation` queues ops and exposes `isPending`. Several controls do
not use it: they call `applyOp` or `ungroupGroup` from `crates/web/ui/src/api/client.ts`
themselves and apply the returned view. Nothing stops those controls from
firing again while the first request is in flight, and each accepted
request is its own timeline entry, so the operator ends up needing two
Undos for one action.

## The pattern

`crates/web/ui/src/hooks/usePendingGuard.ts` returns `{ pendingKeys, runGuarded }`.

```ts
runGuarded(key, () =>
  applyOp(op)
    .then((view) => onViewUpdate(view))
    .catch((err) =>
      onMutationError(err instanceof Error ? err : new Error(String(err))),
    ),
);
```

- **Key by the item, not the direction.** Include and exclude on the same
  item share one key. In `MainContent` the package toggle and the Not
  carried Include share `name.arch`, because both act on the same package.
- **Put the success and error handling inside the request.** The key is
  released in a `finally` after them, on success and on failure, so the
  control re-enables for a retry.
- **Disable the control from `pendingKeys`.** That is what the operator
  sees; the ref inside the hook is what actually drops a second call.

## Why disabling the control is not enough

- A disabled checkbox does not stop a keyboard path to the same handler.
  The quadlet, flatpak and tuned rows toggle on Space or `x` on the row,
  which a disabled checkbox inside it does not block.
- Observed in the vitest suite: two clicks on a PatternFly `Button` inside
  one `act` both reach the handler before the disabled state renders, so
  only the ref drops the second. A controlled checkbox re-renders between
  the two clicks, so its double-click case passes on the disabled state
  alone and does not exercise the ref; the hook's own test does.

## Tests

`components/__tests__/PendingGuards.test.tsx` is a table over the guarded
controls: trigger twice in one `act` with a request held open and assert
one call, then reject the request and assert the control is enabled again.
Add a row when you add a direct-request control.
