---
name: web-ui-direct-request-guard
description: A refine UI control that sends its request directly instead of through App's `useMutation` has no `isPending`, so it must go through `usePendingGuard`, keyed by the full identity of the item it changes, or a double click or a repeated key press sends the request twice.
---

# Direct-request controls need `usePendingGuard`

App's `useMutation` queues ops and exposes `isPending`. Several controls do
not use it: they call a mutating function from `crates/web/ui/src/api/client.ts`
themselves (`applyOp`, `ungroupGroup`, `setUserStrategy`, `setUserPassword`,
or, through App's handlers, `setPackagePin`, `setBulkPackagePin`,
`batchToggleGroup`, `resetDefaults`). Nothing stops those controls from
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
- **The key is the item's full identity.** Build it from every field the
  request uses to name its target, such as the op's `item_id` key. A
  flatpak is `(app_id, remote, branch)`; keying it by `app_id` alone let a
  request on one branch silently drop a toggle on another branch of the
  same app.
- **Encode it unambiguously, in one function.** Write one key function per
  item kind and call it for both `runGuarded` and the disabled lookup.
  Where a hook instance holds more than one kind, or the identity has more
  than one field, use `JSON.stringify([kind, ...fields])`, so no field
  value containing a separator can collide. A single-field identity in an
  instance that holds one kind can be the field itself. `name.arch` is
  safe because an RPM arch never contains a dot.
- **Put the success and error handling inside the request.** The key is
  released in a `finally` after them, on success and on failure, so the
  control re-enables for a retry. If the request factory throws instead of
  returning a promise, the hook releases the key and rethrows.
- **Guard the whole action, not only the request.** Set password hashes in
  the browser before it sends anything, so the hash runs inside the guarded
  request and a second click while hashing is dropped too.
- **When the guard lives below the handler, return the promise.** The pin,
  batch toggle and reset guards live in the component that renders the
  control (`LanguagePackageList`, `Sidebar`, `StatsBar`), and App's handler
  returns its request so the guard is held until it settles.
- **Disable the control from `pendingKeys`.** That is what the operator
  sees; the ref inside the hook is what actually drops a second call.

## Guarded controls and their keys

| Control | Where | Key |
|---|---|---|
| Package checkbox, Not carried Include | `MainContent` | `name.arch` |
| Repo switch | `MainContent` | section id |
| Group toggle, ungroup | `MainContent` | group name |
| Quadlet row | `ContainerSection` | `["quadlet", path]` |
| Flatpak row | `ContainerSection` | `["flatpak", app_id, remote, branch]` |
| Tuned row | `TunedSection` | `tuned:<profile>` |
| User include, strategy, password | `UsersGroupsSection` | username |
| Language package pin | `LanguagePackageList` | `["pin", ecosystem, env_path, package]` |
| Bulk pin | `LanguagePackageList` | `["bulk-pin", ecosystem, env_path]` |
| Batch toggle | `Sidebar` | group or section slug |
| Reset | `StatsBar` | `reset` |

The user's strategy and password share one key because both change the one
user record. A batch toggle or bulk pin does not block the per-item toggles
it covers, and the per-item toggles do not block it: each request sends
explicit values, so they cannot double-apply.

## Why disabling the control is not enough

- A disabled checkbox does not stop a keyboard path to the same handler.
  The quadlet, flatpak and tuned rows toggle on Space or `x` on the row,
  a language package row pins on Enter or Space, and the sidebar batch
  toggles on Ctrl+Shift+A or X. None of those is blocked by a disabled
  control.
- Observed in the vitest suite: two clicks on a PatternFly `Button` inside
  one `act` both reach the handler before the disabled state renders, so
  only the ref drops the second. A controlled checkbox re-renders between
  the two clicks, so its double-click case passes on the disabled state
  alone and does not exercise the ref; the hook's own test does.

## Tests

`components/__tests__/PendingGuards.test.tsx` is a table over the guarded
controls: trigger twice in one `act` with a request held open and assert
one call, then reject the request and assert the control is enabled again.
Add a row when you add a direct-request control. Controls whose request is
a callback prop rather than a client call (pin, bulk pin, batch toggle,
reset) have the same two cases in their own component test files, where
their fixtures live. Where a key has more than one field, add a case with
two items differing in only one field: a held request on one leaves the
other enabled and sending its own request.
