import { Alert } from "@patternfly/react-core";
import type { TmpfilesMissingOwner } from "../api/types";

/**
 * What the operator does about a named line. Mirrors `REMEDY` in
 * `crates/pipeline/src/render/tmpfiles_accounts.rs`, which the
 * Containerfile and both audit reports print, so refine says the same.
 */
export const TMPFILES_OWNER_REMEDY =
  "Create each missing user or group (useradd in refine) or provision it, so " +
  "its line can run. Otherwise systemd-tmpfiles skips the line, and " +
  "systemd-tmpfiles-setup.service still reports success.";

/**
 * The carried tmpfiles.d lines that name an account the image does not
 * create, shown in Configuration Files as the audit reports show them.
 * Renders nothing when there are none.
 */
export function TmpfilesOwnerNotice({
  rows,
}: {
  rows: TmpfilesMissingOwner[] | undefined;
}) {
  if (!rows || rows.length === 0) return null;
  return (
    <Alert
      variant="warning"
      isInline
      title={`tmpfiles.d lines naming accounts the image lacks (${rows.length})`}
      data-testid="tmpfiles-owner-notice"
      style={{ marginBottom: "var(--pf-t--global--spacer--md)" }}
    >
      <p>Each line below names an account the image does not create.</p>
      <ul>
        {rows.map((m, i) => (
          // A file can name one path on several lines, so the index keeps
          // keys unique; the list never reorders in place.
          <li key={`${m.file}\u0000${m.path}\u0000${i}`} data-testid="tmpfiles-owner-row">
            <code>{m.file}</code>: <code>{m.path}</code> needs
            {m.user !== null && (
              <>
                {" "}user <code>{m.user}</code>
              </>
            )}
            {m.user !== null && m.group !== null && ","}
            {m.group !== null && (
              <>
                {" "}group <code>{m.group}</code>
              </>
            )}
          </li>
        ))}
      </ul>
      <p>{TMPFILES_OWNER_REMEDY}</p>
    </Alert>
  );
}
