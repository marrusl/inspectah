import type {
  SysctlViewDto,
  TunedDecisionDto,
  ViewResponse,
} from "../api/types";
import { SysctlPanel } from "./SysctlPanel";
import { TunedSection } from "./TunedSection";
import type { PendingGuard } from "../hooks/usePendingGuard";

export interface SystemTuningSectionProps {
  sysctl: SysctlViewDto;
  tuned: TunedDecisionDto[];
  onViewUpdate: (view: ViewResponse) => void;
  onMutationError: (err: Error) => void;
  /** Opens the Configuration Files row for a sysctl file. */
  onNavigateToConfig?: (configPath: string) => void;
  /** Tuned requests in flight, owned by a component that outlives this one. */
  tunedGuard: PendingGuard;
}

/**
 * The tuned decision rows and the read-only sysctl panel, each shown on
 * its own data. The empty state appears only when there is neither a
 * tuned row nor a sysctl file (spec § 5.6); an aggregate or unavailable
 * sysctl view always shows its line.
 */
export function SystemTuningSection({
  sysctl,
  tuned,
  onViewUpdate,
  onMutationError,
  onNavigateToConfig,
  tunedGuard,
}: SystemTuningSectionProps) {
  const hasTuned = tuned.length > 0;
  const showPanel = sysctl.state !== "host" || sysctl.files.length > 0;

  if (!hasTuned && !showPanel) {
    return (
      <div
        className="inspectah-service-section"
        data-testid="system-tuning-section"
      >
        <p style={{ padding: "var(--pf-t--global--spacer--md)", opacity: 0.6 }}>
          No system tuning detected.
        </p>
      </div>
    );
  }

  return (
    <div data-testid="system-tuning-section">
      {hasTuned && (
        <>
          <h3
            style={{
              fontSize: "var(--pf-t--global--font--size--sm)",
              fontWeight: 600,
              opacity: 0.7,
              padding:
                "var(--pf-t--global--spacer--sm) var(--pf-t--global--spacer--md)",
              margin: 0,
            }}
          >
            Tuned Profiles
          </h3>
          <TunedSection
            tuned={tuned}
            onViewUpdate={onViewUpdate}
            onMutationError={onMutationError}
            pendingGuard={tunedGuard}
          />
        </>
      )}
      {hasTuned && showPanel && (
        <hr
          style={{
            border: "none",
            borderTop: "1px solid var(--pf-t--global--border--color--default)",
            margin:
              "var(--pf-t--global--spacer--sm) var(--pf-t--global--spacer--md)",
          }}
        />
      )}
      {showPanel && (
        <SysctlPanel sysctl={sysctl} onNavigateToConfig={onNavigateToConfig} />
      )}
    </div>
  );
}
