import type { ViewResponse } from "../api/types";
import { mockStats } from "./mockStats";

/**
 * A complete view response with nothing in it: every field the server
 * always sends, each empty. Spread a test's fields over it. A partial
 * object cast to `ViewResponse` hides a field the server requires, which
 * is how a renamed or dropped field reads as an empty section.
 */
export function emptyView(overrides: Partial<ViewResponse> = {}): ViewResponse {
  return {
    packages: [],
    config_files: [],
    containerfile_preview: "",
    stats: mockStats({ sections: [] }),
    generation: 1,
    repo_groups: [],
    version_changes: [],
    unit_states: [],
    unit_dropins: [],
    mount_findings: [],
    quadlets: [],
    flatpaks: [],
    sysctl: { state: "unavailable", reason: "not collected" },
    tuned: [],
    users_groups_decisions: [],
    package_groups: [],
    session_is_sensitive: false,
    not_carried_packages: [],
    not_carried_notice: null,
    ...overrides,
  };
}
