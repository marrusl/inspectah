/** Passive chip display showing counts per triage bucket. */

/** Map bucket name to a human-readable display label. */
function bucketLabel(bucket: string): string {
  const labels: Record<string, string> = {
    investigate: "Investigate",
    site: "Site",
    divergent: "Divergent",
    partial: "Partial",
    baseline: "Baseline",
    universal: "Universal",
    needs_review: "Needs Review",
    informational: "Informational",
    routine: "Routine",
  };
  return labels[bucket] ?? bucket.charAt(0).toUpperCase() + bucket.slice(1);
}

export function TriageStatusBar({
  bucketCounts,
}: {
  bucketCounts: Record<string, number>;
}) {
  return (
    <div className="inspectah-triage-status-bar">
      {Object.entries(bucketCounts).map(([bucket, count]) => (
        <span
          key={bucket}
          className={`inspectah-triage-chip inspectah-triage-chip--${bucket}`}
        >
          {bucketLabel(bucket)}: {count}
        </span>
      ))}
    </div>
  );
}
