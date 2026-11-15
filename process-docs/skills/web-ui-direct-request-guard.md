---
name: web-ui-direct-request-guard
description: A refine UI control that sends its request directly instead of through App's `useMutation` has no `isPending`, so it must go through `usePendingGuard`, keyed by the full identity of the item it changes and owned by a component that stays mounted for the session, or a double click, a repeated key press, or a repeat after navigating away and back sends the request twice.
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
- **Own the guard where it survives navigation.** The guard is React
  state and a ref, so it lasts only as long as the component that calls
  `usePendingGuard`. The components that render these controls do not
  last: `MainContent` mounts only the active section, so switching
  sections unmounts it, and App mounts the mobile sidebar overlay only
  while it is open. A guard owned there comes back empty while its request
  is still in flight, and the same request goes out again. So the owner is
  a component that stays mounted for the session, and the control's
  component takes the guard as a required prop (`pendingGuard`, `pinGuard`,
  `batchGuard`). Required, so a new render site cannot quietly create a
  guard of its own.
- **No pending state of your own around a direct request.** A `useState`
  set of ids or an `isSubmitting` flag next to a direct request has both
  faults this skill exists for: it is not synchronous, so two key presses
  in one tick both send, and it dies with the component. Use the shared
  guard from the stable owner instead.
- **What a request leaves behind must outlive the control too.** A
  failure message held in the component that started the request is lost
  when that component unmounts before the request settles, and the
  operator comes back to a control with no sign that anything failed.
  Keep it with the guard's owner, in `useItemFailures`
  (`crates/web/ui/src/hooks/useItemFailures.ts`). The Users section does
  this: `MainContent` holds each user's last failed request by username,
  and the card shows it on return. The generated timer toggle does the
  same by generated name, and `ContextItem` shows the failure in the
  row's live region. `onMutationError` is not a substitute: in
  `MainContent` it only logs.
- **A kept failure ends with the record it was about.** Kept with a
  long-lived owner, a failure outlives the item too, unless something
  ends it. `useItemFailures` gives each item a lifetime: `clearAll` ends
  every one after a successful reset (App bumps `resetCounter` on
  `MainContent`), `retain` ends the lifetime of every item missing
  from a new view and drops its failure, and `end` ends the lifetime of
  the items an Undo or Redo changed. Start each attempt with
  `begin(key)`: it clears the item's failure and returns the recorder for
  this attempt, which records nothing once the lifetime it started in has
  ended. A request in flight across a reset, an Undo or Redo that
  changes the item, or the item leaving the view and coming back,
  settles against a record it never changed. Section navigation and
  ordinary refreshes do not end a lifetime, so a failure still survives
  a section switch.
- **Undo and Redo end the lifetimes of the records they change.** A step
  through history keeps the item in the view but can replace its record.
  `useMutation` tells its success callback which kind of request
  produced the view; on an Undo or Redo, App compares each user record in
  the view on screen with the view the step returned (`changedUsers` in
  `components/userHistory.ts`) and passes the changed names to
  `MainContent` as `historyChange`, which calls `end` on them. Users the
  step did not change keep their failures. Compare records rather than
  read the undone op from `/api/ops`: that is a separate request that
  can fail, and it would need per-op knowledge of which item an op
  touches.
- **Announce a failure that lands asynchronously.** The operator's focus
  is rarely on the message when a request fails, so plain styled text is
  silent to a screen reader. Put the message in PatternFly error helper
  text (`HelperTextItem variant="error"`) inside a `HelperText
  isLiveRegion` that stays in the page while its container is open, and
  render only the item conditionally. A region that is already present
  announces the change once; a container that mounts with a failure
  already showing, after a section switch, does not announce it again.
  Use one region per message: nesting a live region in an alert, or
  adding `role="alert"` to a live region, reads it twice.
- **A form that can open with a failure is not the announcer.** A region
  inserted with its content is not reliably announced, and a new-password
  failure that lands after the operator closed the form opens the form
  with the message already in it. The user card has one live region that
  stays mounted with the card: it shows strategy and password-option
  failures, and carries a visually hidden copy of the password form's
  message (`inspectah-sr-only`, prefixed with the user name) while the
  form is open. The form's own copy is plain error helper text, not live.
- **Closing what shows a failure acknowledges it.** A failure that opens
  its form when the card mounts reopens it on every return unless
  dismissing it clears it. The user card clears a new-password failure
  when the operator closes the card or the password form while it shows.
- **Do not let a close abandon a request you cannot cancel.** A modal that
  stays mounted while closed keeps its state, so resetting its busy flag
  on close lets a second request start while the first still runs, and
  the first one's result lands in the next opening. The RPM upload modal
  blocks every close path while uploading (no `onClose` on the PatternFly
  `Modal`, which withdraws the close button and ignores Escape) and
  checks a ref before it sends, so a double click sends one upload.
- **When the guard lives above the request, return the promise.** The pin,
  batch toggle and reset requests are App's handlers, called through a
  callback prop. Each handler returns its request so the guard is held
  until it settles.
- **Disable the control from `pendingKeys`.** That is what the operator
  sees; the ref inside the hook is what actually drops a second call.

## Guarded controls and their keys

| Control | Rendered in | Guard owner | Key |
|---|---|---|---|
| Package checkbox, Not carried Include | `MainContent` | `MainContent` | `name.arch` |
| Repo switch | `MainContent` | `MainContent` | section id |
| Group toggle, ungroup | `MainContent` | `MainContent` | group name |
| Quadlet row | `ContainerSection` | `MainContent` | `["quadlet", path]` |
| Flatpak row | `ContainerSection` | `MainContent` | `["flatpak", app_id, remote, branch]` |
| Tuned row | `TunedSection` | `MainContent` | `tuned:<profile>` |
| Service row | `ServiceSection` | `MainContent` | `["service", unit]` |
| Service drop-in row | `ServiceSection` | `MainContent` | `["dropin", path]` |
| Generated timer row | `ContextItem` (Scheduled Tasks) | `MainContent` | `["generated-timer", name]` |
| User include, strategy, password | `UsersGroupsSection` | `MainContent` | username |
| Language package pin | `LanguagePackageList` | `MainContent` | `["pin", ecosystem, env_path, package]` |
| Bulk pin | `LanguagePackageList` | `MainContent` | `["bulk-pin", ecosystem, env_path]` |
| Batch toggle | `Sidebar` | App | group or section slug |
| Reset | `StatsBar` | `StatsBar` | `reset` |

`MainContent` is rendered unconditionally and without a `key`, so it stays
mounted for the session: it switches sections by what it renders, not by
remounting. App holds the batch guard because neither sidebar stays
mounted: the overlay unmounts on close, and the desktop sidebar unmounts
when the viewport narrows. `StatsBar` is rendered unconditionally by
`AppShell`, so it keeps its own reset guard. If any of these owners ever
gets a `key` or a conditional mount, move its guards up.

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
Add a row when you add a direct-request control. The section rows render
through `MainContent`, their production owner. Controls whose request is
a callback prop rather than a client call (pin, bulk pin, batch toggle,
reset) have the same two cases in their own component test files, where
their fixtures live; those tests render the component through a wrapper in
`src/test-utils/withPendingGuard.tsx` that stands in for its owner.

Each guard owned above its control also has a lifetime case: hold a
request open, unmount the control's component and mount it again, and
assert the control is still disabled and a repeat is still dropped until
the request settles. The section cases switch `MainContent` to another
section and back (`PendingGuards.test.tsx`), and Set password has the
same switch with a failure landing while the operator is away, asserting
the failure shows on return; the batch toggle case closes
and reopens the mobile overlay in a full `App` (`src/__tests__/BatchToggle.guard.test.tsx`). Where a key has more than one field, add a case with
two items differing in only one field: a held request on one leaves the
other enabled and sending its own request.

Kept failures have their own cases in
`components/__tests__/MainContent.user-failures.test.tsx`: a failure
survives a new view that still has the item and a section switch; a
reset clears it; an item that leaves the view and comes back shows none;
a request held across a reset, or across the item leaving and coming
back, records nothing when it fails; closing the form survives a
rerender and a switch, and a new failure opens it again; and each
failure kind shows both when it lands on a mounted card and when it
lands while the operator is away, until a successful retry clears it.
The same file covers `historyChange` (a shown failure clears when the
step changed the user, another user's failure stays, a held request that
fails after the step records nothing) and checks that each failure lands
in the card's polite live region, which was in the page before it, that
the card has no other announcer, and that a password failure landing
after the form was closed is announced from that same region.
`src/__tests__/UserFailures.reset.test.tsx` runs the reset case in a
full `App`, so it also covers App bumping `resetCounter`, and
`src/__tests__/UserFailures.history.test.tsx` runs Undo in a full `App`
with one changed and one unchanged user, so it covers the record
comparison. `components/__tests__/RpmUploadModal.test.tsx` holds an
upload and settles it both ways: a refusal must leave the modal able to
upload again and to close.
