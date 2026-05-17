export interface AttentionSummaryProps {
  needsReviewCount: number;
  needsReviewRepoCount: number;
  infoCount: number;
  infoRepoCount: number;
}

export function AttentionSummary({
  needsReviewCount,
  needsReviewRepoCount,
  infoCount,
  infoRepoCount,
}: AttentionSummaryProps) {
  let text: string;

  if (needsReviewCount > 0) {
    const pkgWord = needsReviewCount === 1 ? "package" : "packages";
    const verb = needsReviewCount === 1 ? "needs" : "need";
    const repoWord = needsReviewRepoCount === 1 ? "repo" : "repos";
    text = `${needsReviewCount} ${pkgWord} ${verb} review across ${needsReviewRepoCount} ${repoWord}`;
  } else if (infoCount > 0) {
    const repoWord = infoRepoCount === 1 ? "repo" : "repos";
    text = `No packages flagged for review · ${infoCount} informational across ${infoRepoCount} ${repoWord}`;
  } else {
    text = "All actionable items reviewed";
  }

  return (
    <div
      data-testid="attention-summary"
      style={{
        padding: "var(--pf-t--global--spacer--sm) 0",
        fontSize: "var(--pf-t--global--font--size--body--default)",
        color: needsReviewCount > 0
          ? "var(--pf-t--global--color--status--danger--default)"
          : "var(--pf-t--global--text--color--subtle)",
        fontWeight: needsReviewCount > 0 ? 600 : 400,
      }}
    >
      {text}
    </div>
  );
}
