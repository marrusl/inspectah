import type { SysctlDecisionDto, TunedDecisionDto, ViewResponse } from "../api/types";
import { SysctlSection } from "./SysctlSection";
import { TunedSection } from "./TunedSection";

export interface SystemTuningSectionProps {
  sysctls: SysctlDecisionDto[];
  tuned: TunedDecisionDto[];
  onViewUpdate: (view: ViewResponse) => void;
  onMutationError: (err: Error) => void;
}

export function SystemTuningSection({
  sysctls,
  tuned,
  onViewUpdate,
  onMutationError,
}: SystemTuningSectionProps) {
  const hasSysctls = sysctls.length > 0;
  const hasTuned = tuned.length > 0;

  if (!hasSysctls && !hasTuned) {
    return (
      <div
        className="inspectah-service-section"
        data-testid="system-tuning-section"
      >
        <p style={{ padding: "var(--pf-t--global--spacer--md)", opacity: 0.6 }}>
          No system tuning overrides detected.
        </p>
      </div>
    );
  }

  return (
    <div data-testid="system-tuning-section">
      {hasSysctls && (
        <>
          <h3
            style={{
              fontSize: "var(--pf-t--global--font--size--sm)",
              fontWeight: 600,
              opacity: 0.7,
              padding: "var(--pf-t--global--spacer--sm) var(--pf-t--global--spacer--md)",
              margin: 0,
            }}
          >
            Sysctls
          </h3>
          <SysctlSection
            sysctls={sysctls}
            onViewUpdate={onViewUpdate}
            onMutationError={onMutationError}
          />
        </>
      )}
      {hasSysctls && hasTuned && (
        <hr
          style={{
            border: "none",
            borderTop: "1px solid var(--pf-t--global--border--color--default)",
            margin: "var(--pf-t--global--spacer--sm) var(--pf-t--global--spacer--md)",
          }}
        />
      )}
      {hasTuned && (
        <>
          <h3
            style={{
              fontSize: "var(--pf-t--global--font--size--sm)",
              fontWeight: 600,
              opacity: 0.7,
              padding: "var(--pf-t--global--spacer--sm) var(--pf-t--global--spacer--md)",
              margin: 0,
            }}
          >
            Tuned Profiles
          </h3>
          <TunedSection
            tuned={tuned}
            onViewUpdate={onViewUpdate}
            onMutationError={onMutationError}
          />
        </>
      )}
    </div>
  );
}
