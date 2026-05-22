import { useState, useEffect, useCallback } from "react";
import { Page, PageSection, EmptyState, EmptyStateBody, Spinner } from "@patternfly/react-core";
import type { FleetHealthInfo, HealthResponse, FleetViewResponse } from "../api/types";
import { fetchFleetView } from "../api/fleet-client";
import { useFleetMutation } from "../hooks/useFleetMutation";
import { useVariantAck } from "../hooks/useVariantAck";
import { useFleetFocusRecovery } from "../hooks/useFleetFocusRecovery";
import { AppShell } from "./AppShell";
import { FleetSidebar } from "./fleet/FleetSidebar";

export interface FleetAppProps {
  fleet: FleetHealthInfo;
  health: HealthResponse;
}

/** Toolbar indicator showing unacked variant count. */
function AckProgress({ unackedCount, totalCount }: { unackedCount: number; totalCount: number }) {
  if (totalCount === 0) return null;
  return (
    <span className="fleet-ack-progress" data-testid="ack-progress">
      {unackedCount} of {totalCount} variants need review
    </span>
  );
}

export function FleetApp({ fleet, health: _health }: FleetAppProps) {
  const [view, setView] = useState<FleetViewResponse | null>(null);
  const [activeSection, setActiveSection] = useState("packages");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    fetchFleetView().then(setView).catch((e) => setError(e.message));
  }, []);

  const { undo, redo, isPending, refetchError, retry } = useFleetMutation(
    setView,
    (err) => setError(err.message),
  );

  const actionableIds = view?.summary.actionable_variant_items.map((v) => v.item_id) ?? [];
  const ack = useVariantAck(fleet.label, fleet.merged_at, actionableIds);

  // Restore focus to the last-focused fleet item after view updates
  useFleetFocusRecovery(view?.generation ?? null);

  const handleSearchNavigate = useCallback(
    (sectionId: string, _itemId: string) => {
      setActiveSection(sectionId);
    },
    [],
  );

  // Loading state
  if (!view && !error) {
    return (
      <Page className="inspectah-page" data-testid="fleet-app">
        <PageSection>
          <EmptyState titleText="Loading fleet view..." headingLevel="h2">
            <Spinner size="xl" />
          </EmptyState>
        </PageSection>
      </Page>
    );
  }

  // Error state (no data at all)
  if (error && !view) {
    return (
      <Page className="inspectah-page" data-testid="fleet-app">
        <PageSection>
          <EmptyState titleText="Failed to load fleet view" headingLevel="h2">
            <EmptyStateBody>{error}</EmptyStateBody>
          </EmptyState>
        </PageSection>
      </Page>
    );
  }

  // view is guaranteed non-null past this point
  const fleetView = view!;

  return (
    <div data-testid="fleet-app">
      <AppShell
        sidebar={
          <FleetSidebar
            sections={fleetView.sections}
            activeSection={activeSection}
            onSelect={setActiveSection}
            ackState={ack}
          />
        }
        containerfilePreview={fleetView.containerfile_preview}
        stats={null}
        generation={fleetView.generation}
        sessionIsSensitive={fleetView.session_is_sensitive}
        onUndo={undo}
        onRedo={redo}
        onExportComplete={() => {
          fetchFleetView().then(setView);
        }}
        isPending={isPending}
        activeSection={activeSection}
        onNavigateSection={setActiveSection}
        searchPackageItems={[]}
        searchConfigItems={[]}
        searchContextSections={null}
        onSearchNavigate={handleSearchNavigate}
        toolbarExtra={<AckProgress unackedCount={ack.unackedCount} totalCount={ack.totalCount} />}
        extraShortcuts={[{ key: "c", description: "Compare variants" }]}
      >
        {(_shellState) => (
          <div className="fleet-content" data-testid="fleet-content">
            <div>Active section: {activeSection}</div>
            <div>Sections: {fleetView.sections.length}</div>
            {refetchError && (
              <div className="refetch-error" data-testid="refetch-error">
                {refetchError}
                <button onClick={retry}>Retry</button>
              </div>
            )}
          </div>
        )}
      </AppShell>
    </div>
  );
}
