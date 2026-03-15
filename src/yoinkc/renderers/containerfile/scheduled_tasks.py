"""Containerfile section: scheduled tasks (timers, cron, at jobs)."""

from ...schema import InspectionSnapshot


def section_lines(snapshot: InspectionSnapshot) -> list[str]:
    """Return Containerfile lines for scheduled tasks."""
    lines: list[str] = []

    st = snapshot.scheduled_tasks
    if st and (st.generated_timer_units or st.systemd_timers or st.cron_jobs or st.at_jobs):
        lines.append("# === Scheduled Tasks ===")

        local_timers = [t for t in st.systemd_timers if t.source == "local"]
        vendor_timers = [t for t in st.systemd_timers if t.source == "vendor"]
        included_timers = [u for u in st.generated_timer_units if u.include]

        # Timer unit files must be present before systemctl enable runs.
        # The later consolidated COPY config/etc/ /etc/ re-copies them harmlessly.
        if local_timers or included_timers:
            lines.append("COPY config/etc/systemd/system/ /etc/systemd/system/")

        if local_timers:
            lines.append(f"# Existing local timers ({len(local_timers)}): "
                         + ", ".join(f"{t.name}.timer" for t in local_timers))

        if included_timers:
            lines.append(f"# Converted from cron: {len(included_timers)} timer(s): "
                         + ", ".join(u.name for u in included_timers if u.name))

        # Consolidate all timer enables into a single RUN (one layer, matching
        # the pattern used by the Services section).
        timer_names_to_enable = (
            [f"{t.name}.timer" for t in local_timers]
            + [f"{u.name}.timer" for u in included_timers if u.name]
        )
        if timer_names_to_enable:
            lines.append("RUN systemctl enable " + " ".join(timer_names_to_enable))

        if st.at_jobs:
            lines.append(f"# FIXME: {len(st.at_jobs)} at job(s) found — convert to systemd timers or cron")
            for a in st.at_jobs:
                lines.append(f"#   at job: {a.command}")

        lines.append("")

    return lines
