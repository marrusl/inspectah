import type { FleetHealthInfo, HealthResponse } from "../api/types";

export interface FleetAppProps {
  fleet: FleetHealthInfo;
  health: HealthResponse;
}

/**
 * Placeholder for fleet mode UI.
 * Real implementation is Task 9.
 */
export function FleetApp({ fleet }: FleetAppProps) {
  return (
    <div data-testid="fleet-app">
      Fleet mode — {fleet.host_count} hosts
    </div>
  );
}
