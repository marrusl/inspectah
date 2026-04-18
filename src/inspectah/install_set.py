"""Shared install-set resolution used by both preflight and the packages renderer.

resolve_install_set() applies the same filters the renderer uses:
  1. p.include filter (user exclusions)
  2. Leaf-package filter (when baseline exists, only explicit installs)
  3. Shell safety filter (reject names with shell metacharacters)
  4. Synthetic prerequisite injection (e.g., tuned)

Both the preflight module and the renderer call this function so they
always operate on the same package list.
"""

from typing import List

from .renderers.containerfile._helpers import _sanitize_shell_value, _TUNED_PROFILE_RE
from .schema import InspectionSnapshot


def resolve_install_set(snapshot: InspectionSnapshot) -> List[str]:
    """Return the sorted, deduplicated list of package names to install.

    This is the exact set the renderer will emit in the ``dnf install``
    line and the preflight module will validate against target repos.
    """
    rpm = snapshot.rpm
    result: List[str] = []

    if rpm and rpm.packages_added:
        # 1. Include filter
        included = [p for p in rpm.packages_added if p.include]
        raw_names = sorted(set(p.name for p in included))

        # 2. Shell safety filter
        safe_names = [n for n in raw_names if _sanitize_shell_value(n, "dnf install") is not None]

        # 3. Leaf filter (only when baseline exists)
        leaf_set = set(rpm.leaf_packages) if rpm.leaf_packages is not None else None
        if leaf_set is not None and not getattr(rpm, "no_baseline", False):
            included_name_set = set(raw_names)
            included_leaf_names = leaf_set & included_name_set
            result = sorted(n for n in safe_names if n in included_leaf_names)
        else:
            result = safe_names

    # 4. Synthetic prerequisite: tuned (even when no packages exist)
    needs_tuned = bool(
        snapshot.kernel_boot and snapshot.kernel_boot.tuned_active
        and _TUNED_PROFILE_RE.match(snapshot.kernel_boot.tuned_active)
    )
    if needs_tuned and "tuned" not in result:
        result = sorted(result + ["tuned"])

    return result
