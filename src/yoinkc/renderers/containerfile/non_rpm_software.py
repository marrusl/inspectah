"""Containerfile section: non-RPM software (pip, go, standalone)."""

from ...schema import InspectionSnapshot


def section_lines(
    snapshot: InspectionSnapshot,
    *,
    pure_pip: list,
    needs_multistage: bool,
) -> list[str]:
    """Return Containerfile lines for non-RPM software."""
    lines: list[str] = []

    if not (snapshot.non_rpm_software and snapshot.non_rpm_software.items):
        return lines

    lines.append("# === Non-RPM Software ===")

    # Determine which tool packages are actually needed by items in this section.
    # If they're not already guaranteed by the dnf install block (i.e., not in
    # leaf_packages or packages_added), emit a prerequisite install up front.
    _installed_names: set = set()
    if snapshot.rpm:
        if snapshot.rpm.packages_added:
            _installed_names.update(p.name for p in snapshot.rpm.packages_added if p.include)
        if snapshot.rpm.leaf_packages:
            _installed_names.update(snapshot.rpm.leaf_packages)
        if snapshot.rpm.auto_packages:
            _installed_names.update(snapshot.rpm.auto_packages)

    _included_items = [it for it in snapshot.non_rpm_software.items if it.include]
    _needs_nodejs = any(it.method in ("npm package-lock.json", "yarn.lock")
                        for it in _included_items)
    # python venv uses the venv's own pip — system python3-pip only needed for
    # bare "pip install" calls (requirements.txt and non-C-extension dist-info).
    _needs_pip = any(it.method == "pip requirements.txt" for it in _included_items) or any(
        it.method == "pip dist-info" and it.version and not it.has_c_extensions
        for it in _included_items
    )

    _prereq_pkgs: list = []
    if _needs_nodejs and not (_installed_names & {"npm", "nodejs"}):
        # RHEL 9: npm is a separate package; RHEL 10: bundled in nodejs.
        # The || fallback handles both distro generations.
        _prereq_pkgs.append("nodejs npm || dnf install -y nodejs")
    if _needs_pip and "python3-pip" not in _installed_names:
        _prereq_pkgs.append("python3-pip")

    if _prereq_pkgs:
        lines.append("# Tool prerequisites not in the dnf install block above:")
        if len(_prereq_pkgs) == 1 and "||" in _prereq_pkgs[0]:
            lines.append(f"RUN dnf install -y {_prereq_pkgs[0]}")
        else:
            non_fallback = [p for p in _prereq_pkgs if "||" not in p]
            fallback = [p for p in _prereq_pkgs if "||" in p]
            if non_fallback:
                lines.append("RUN dnf install -y " + " ".join(non_fallback))
            for fb in fallback:
                lines.append(f"RUN dnf install -y {fb}")
        lines.append("")

    pip_packages: list = []
    remaining: list = []

    for item in snapshot.non_rpm_software.items:
        if not item.include:
            continue
        method = item.method
        lang = item.lang
        path = item.path or item.name

        if lang in ("go", "rust"):
            linking = "statically linked" if item.static else "dynamically linked"
            lines.append(f"# FIXME: {lang.capitalize()} binary at /{path} ({linking})")
            lines.append(f"# Obtain source and rebuild for the target image, or COPY the binary directly")
            lines.append(f"# COPY config/{path} /{path}")
        elif lang == "c/c++":
            if item.static:
                lines.append(f"# FIXME: static C/C++ binary at /{path} — COPY or rebuild from source")
                lines.append(f"# COPY config/{path} /{path}")
            else:
                libs = ", ".join(item.shared_libs[:5])
                lines.append(f"# FIXME: dynamic C/C++ binary at /{path} — needs: {libs}")
                lines.append(f"# COPY config/{path} /{path}")
        elif method == "python venv":
            pkgs = item.packages
            if item.system_site_packages:
                lines.append(f"# FIXME: venv at /{path} uses --system-site-packages — verify RPM deps are in base image")
            if pkgs:
                lines.append(f"# Python venv at /{path}: {len(pkgs)} package(s)")
                lines.append(f"RUN python3 -m venv /{path}")
                pkg_specs = " ".join(f"{p.name}=={p.version}" for p in pkgs if p.version)
                if pkg_specs:
                    lines.append(f"RUN /{path}/bin/pip install {pkg_specs}")
            else:
                lines.append(f"# FIXME: venv at /{path} — no packages detected, verify manually")
        elif method == "git repository":
            lines.append(f"# Git-managed: /{path}")
            if item.git_remote:
                lines.append(f"# FIXME: clone from {item.git_remote} (branch: {item.git_branch}, commit: {item.git_commit[:12]})")
                lines.append(f"# RUN git clone {item.git_remote} /{path} && cd /{path} && git checkout {item.git_commit[:12]}")
            else:
                lines.append(f"# FIXME: git repo at /{path} has no remote — COPY or reconstruct")
        elif method == "pip dist-info" and item.version:
            if not item.has_c_extensions:
                pip_packages.append((item.name, item.version))
        elif method == "pip requirements.txt":
            lines.append(f"# FIXME: verify pip packages in /{path} install correctly from PyPI")
            lines.append(f"COPY config/{path} /{path}")
            lines.append(f"RUN pip install -r /{path}")
        elif method == "npm package-lock.json":
            lines.append(f"# FIXME: verify npm packages in /{path} install correctly")
            lines.append(f"COPY config/{path}/ /{path}/")
            lines.append(f"RUN cd /{path} && npm ci")
        elif method == "yarn.lock":
            lines.append(f"# FIXME: verify yarn packages in /{path} install correctly")
            lines.append(f"COPY config/{path}/ /{path}/")
            lines.append(f"RUN cd /{path} && yarn install --frozen-lockfile")
        elif method == "gem Gemfile.lock":
            lines.append(f"# FIXME: verify Ruby gems in /{path} install correctly")
            lines.append(f"COPY config/{path}/ /{path}/")
            lines.append(f"RUN cd /{path} && bundle install")
        else:
            remaining.append(item)

    if pip_packages:
        pip_packages.sort()
        lines.append(f"# Detected: {len(pip_packages)} pip package(s) via dist-info")
        lines.append("# FIXME: verify these pip packages install correctly from PyPI")
        lines.append("RUN pip install \\")
        for name, ver in pip_packages[:-1]:
            lines.append(f"    {name}=={ver} \\")
        name, ver = pip_packages[-1]
        lines.append(f"    {name}=={ver}")

    for item in remaining[:20]:
        path = item.path or item.name
        lines.append(f"# FIXME: unknown provenance — determine upstream source and installation method for /{path}")
        lines.append(f"# COPY config/{path} /{path}")

    lines.append("")

    return lines
