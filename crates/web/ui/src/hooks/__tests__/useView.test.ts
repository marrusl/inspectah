import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, waitFor, act } from "@testing-library/react";
import { useView } from "../useView";
import type { ViewResponse } from "../../api/types";

const MOCK_VIEW: ViewResponse = {
  packages: [],
  config_files: [],
  containerfile_preview: "FROM ubi9\n",
  stats: {
    sections: [
      { kind: "package", total: 10, included: 8, excluded: 2 },
      { kind: "config", total: 5, included: 3, excluded: 2 },
    ],
    needs_review_count: 3,
    ops_applied: 1,
    can_undo: true,
    can_redo: false,
    baseline_available: false,
  },
  generation: 1,
  repo_groups: [],
  version_changes: [],
  unit_states: [],
  unit_dropins: [],
  mount_findings: [],
  manual_links: [],
  undetermined_installs: [],
  quadlets: [],
  flatpaks: [],
  declined_generated: [],
  generated_timers: [],
  sysctl: { state: "unavailable", reason: "not collected" },
  tuned: [],
  users_groups_decisions: [],
  package_groups: [],
  session_is_sensitive: false,
  not_carried_packages: [],
  not_carried_notice: null,
  machine_state: [],
  machine_state_steps: [],
  machine_state_notices: [],
};

beforeEach(() => {
  vi.restoreAllMocks();
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("useView", () => {
  it("returns loading initially", () => {
    vi.spyOn(globalThis, "fetch").mockImplementation(
      () => new Promise(() => {}), // never resolves
    );
    const { result } = renderHook(() => useView());
    expect(result.current.loading).toBe(true);
    expect(result.current.data).toBeNull();
    expect(result.current.error).toBeNull();
  });

  it("returns data on success", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: true,
      json: () => Promise.resolve(MOCK_VIEW),
    } as Response);

    const { result } = renderHook(() => useView());
    await waitFor(() => expect(result.current.loading).toBe(false));

    expect(result.current.data).toEqual(MOCK_VIEW);
    expect(result.current.error).toBeNull();
  });

  it("returns error on failure", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: false,
      status: 500,
      json: () => Promise.resolve({ error: "server error" }),
    } as unknown as Response);

    const { result } = renderHook(() => useView());
    await waitFor(() => expect(result.current.loading).toBe(false));

    expect(result.current.data).toBeNull();
    expect(result.current.error).toBeTruthy();
  });

  it("refetch triggers a new fetch", async () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: true,
      json: () => Promise.resolve(MOCK_VIEW),
    } as Response);

    const { result } = renderHook(() => useView());
    await waitFor(() => expect(result.current.loading).toBe(false));

    expect(fetchSpy).toHaveBeenCalledTimes(1);

    act(() => {
      result.current.refetch();
    });

    await waitFor(() => expect(fetchSpy).toHaveBeenCalledTimes(2));
  });

  it("publish shows a view at once, and a fetch already out cannot replace it", async () => {
    let answer: (view: ViewResponse) => void = () => {};
    const fetchSpy = vi.spyOn(globalThis, "fetch").mockImplementation(
      () =>
        new Promise((resolve) => {
          answer = (view) =>
            resolve({
              ok: true,
              json: () => Promise.resolve(view),
            } as Response);
        }),
    );

    const { result } = renderHook(() => useView());
    const published = { ...MOCK_VIEW, generation: 5 };
    act(() => {
      result.current.publish(published);
    });
    expect(result.current.data).toEqual(published);
    expect(result.current.loading).toBe(false);

    await act(async () => answer({ ...MOCK_VIEW, generation: 4 }));
    expect(result.current.data).toEqual(published);
    expect(result.current.loading).toBe(false);
    expect(fetchSpy).toHaveBeenCalledTimes(1);
  });

  it("an older publish arriving after a newer fetch leaves the newer view shown", async () => {
    const answers = holdFetches();
    const { result } = renderHook(() => useView());
    await act(async () => answers[0]({ ...MOCK_VIEW, generation: 42 }));

    act(() => result.current.invalidate());
    await waitFor(() => expect(answers).toHaveLength(2));
    const newer = { ...MOCK_VIEW, generation: 43, containerfile_preview: "43" };
    await act(async () => answers[1](newer));
    expect(result.current.data).toEqual(newer);

    act(() => {
      result.current.publish({ ...MOCK_VIEW, generation: 42 });
    });
    expect(result.current.data).toEqual(newer);
  });

  it("a newer fetch answering after a publish replaces the published view", async () => {
    const answers = holdFetches();
    const { result } = renderHook(() => useView());
    await act(async () => answers[0]({ ...MOCK_VIEW, generation: 41 }));

    act(() => result.current.invalidate());
    await waitFor(() => expect(answers).toHaveLength(2));
    act(() => {
      result.current.publish({ ...MOCK_VIEW, generation: 42 });
    });

    const newer = { ...MOCK_VIEW, generation: 43, containerfile_preview: "43" };
    await act(async () => answers[1](newer));
    expect(result.current.data).toEqual(newer);
    expect(result.current.loading).toBe(false);
  });

  it("at an equal generation, a fetch started before the last fetch or publish does not replace it", async () => {
    const answers = holdFetches();
    const { result } = renderHook(() => useView());
    await act(async () => answers[0]({ ...MOCK_VIEW, generation: 7 }));

    act(() => result.current.invalidate());
    await waitFor(() => expect(answers).toHaveLength(2));
    const published = {
      ...MOCK_VIEW,
      generation: 7,
      containerfile_preview: "published",
    };
    act(() => {
      result.current.publish(published);
    });

    await act(async () =>
      answers[1]({
        ...MOCK_VIEW,
        generation: 7,
        containerfile_preview: "fetched",
      }),
    );
    expect(result.current.data).toEqual(published);
  });
});

/** Hold every view request; `answers[i]` answers the i-th one. */
function holdFetches(): ((view: ViewResponse) => void)[] {
  const answers: ((view: ViewResponse) => void)[] = [];
  vi.spyOn(globalThis, "fetch").mockImplementation(
    () =>
      new Promise((resolve) => {
        answers.push((view) =>
          resolve({
            ok: true,
            json: () => Promise.resolve(view),
          } as Response),
        );
      }),
  );
  return answers;
}
