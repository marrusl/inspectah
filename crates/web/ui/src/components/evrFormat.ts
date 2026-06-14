export function formatEvrPair(
  baseEpoch: string,
  baseVersion: string,
  hostEpoch: string,
  hostVersion: string,
): [string, string] {
  const norm = (e: string) => (e === "" ? "0" : e);
  const baseNorm = norm(baseEpoch);
  const hostNorm = norm(hostEpoch);
  const showEpoch = baseNorm !== hostNorm || baseNorm !== "0";

  const fmt = (epoch: string, version: string) => {
    if (showEpoch) {
      const e = epoch === "" ? "0" : epoch;
      return `${e}:${version}`;
    }
    return version;
  };

  return [fmt(baseEpoch, baseVersion), fmt(hostEpoch, hostVersion)];
}
