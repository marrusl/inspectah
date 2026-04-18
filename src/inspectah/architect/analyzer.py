"""Cross-fleet analyzer for layer topology decomposition."""

from __future__ import annotations

from collections import defaultdict
from dataclasses import dataclass, field


@dataclass
class FleetInput:
    """Simplified fleet data for analysis."""

    name: str
    packages: list[str]
    configs: list[str]
    host_count: int = 0
    base_image: str = ""
    unavailable_packages: list[str] = field(default_factory=list)
    direct_install_packages: list[str] = field(default_factory=list)
    unverifiable_packages: list[str] = field(default_factory=list)
    preflight_status: str = "skipped"


@dataclass
class FleetInfo:
    """Fleet metadata for the topology."""

    name: str
    host_count: int
    total_packages: int


@dataclass
class Layer:
    """A layer in the image topology."""

    name: str
    parent: str | None
    packages: list[str] = field(default_factory=list)
    configs: list[str] = field(default_factory=list)
    fleets: list[str] = field(default_factory=list)
    fan_out: int = 0
    turbulence: float = 0.0

    def _recalc_turbulence(self) -> None:
        raw = self.fan_out * (len(self.packages) / 50.0)
        if self.parent is not None:  # non-base layers get floor of 1.0
            self.turbulence = max(1.0, raw)
        else:
            self.turbulence = raw


@dataclass
class LayerTopology:
    """Complete layer topology with move and export support."""

    layers: list[Layer] = field(default_factory=list)
    fleets: list[FleetInfo] = field(default_factory=list)

    def get_layer(self, name: str) -> Layer | None:
        for layer in self.layers:
            if layer.name == name:
                return layer
        return None

    def move_package(self, package: str, from_layer: str, to_layer: str) -> None:
        """Move a package between layers.

        Standard move: remove from from_layer, add to to_layer.
        Special case — moving FROM base: package is broadcast to ALL
        derived layers (every fleet still needs it). The to_layer parameter
        identifies the user's chosen target; broadcast is a side effect.
        """
        src = self.get_layer(from_layer)
        dst = self.get_layer(to_layer)
        if src is None:
            raise ValueError(f"Layer {from_layer!r} not found")
        if dst is None:
            raise ValueError(f"Layer {to_layer!r} not found")
        if package not in src.packages:
            raise ValueError(f"Package {package!r} not found in layer {from_layer!r}")

        src.packages.remove(package)

        if from_layer == "base":
            # Broadcast to ALL derived layers
            for layer in self.layers:
                if layer.parent is not None and package not in layer.packages:
                    layer.packages.append(package)
        else:
            if package not in dst.packages:
                dst.packages.append(package)

        self._recalc_all()

    def copy_package(self, package: str, from_layer: str, to_layer: str) -> None:
        """Copy a package to another layer without removing from source."""
        src = self.get_layer(from_layer)
        dst = self.get_layer(to_layer)
        if src is None:
            raise ValueError(f"Layer {from_layer!r} not found")
        if dst is None:
            raise ValueError(f"Layer {to_layer!r} not found")
        if package not in src.packages:
            raise ValueError(f"Package {package!r} not found in layer {from_layer!r}")
        if package not in dst.packages:
            dst.packages.append(package)
        self._recalc_all()

    def _recalc_all(self) -> None:
        base = self.get_layer("base")
        if base is not None:
            base.fan_out = sum(1 for l in self.layers if l.parent == "base")
            base._recalc_turbulence()
        for layer in self.layers:
            if layer.parent is not None:
                layer.fan_out = sum(1 for l in self.layers if l.parent == layer.name)
                layer._recalc_turbulence()

    def to_dict(self) -> dict:
        return {
            "layers": [
                {
                    "name": l.name,
                    "parent": l.parent,
                    "packages": l.packages,
                    "configs": l.configs,
                    "fleets": l.fleets,
                    "fan_out": l.fan_out,
                    "turbulence": round(l.turbulence, 1),
                }
                for l in self.layers
            ],
            "fleets": [
                {"name": f.name, "host_count": f.host_count, "total_packages": f.total_packages}
                for f in self.fleets
            ],
        }


def analyze_fleets(fleets: list[FleetInput]) -> LayerTopology:
    """Analyze multiple fleets and produce a layer topology.

    Uses 100% cross-fleet prevalence heuristic: packages in ALL fleets -> base.
    Remaining packages stay in their fleet's derived layer.
    Configs always stay with their original fleet (not decomposed).
    Single fleet -> no base extraction (fleet becomes the only layer).
    """
    fleet_names = [f.name for f in fleets]
    fleet_infos = [
        FleetInfo(name=f.name, host_count=f.host_count, total_packages=len(f.packages))
        for f in fleets
    ]

    if len(fleets) == 1:
        f = fleets[0]
        layer = Layer(
            name=f.name,
            parent=None,
            packages=list(f.packages),
            configs=list(f.configs),
            fleets=[f.name],
        )
        layer._recalc_turbulence()
        return LayerTopology(layers=[layer], fleets=fleet_infos)

    # Build cross-fleet package index
    pkg_to_fleets: dict[str, set[str]] = defaultdict(set)
    for f in fleets:
        for pkg in f.packages:
            pkg_to_fleets[pkg].add(f.name)

    all_fleet_names = set(fleet_names)
    base_packages = sorted(pkg for pkg, f_set in pkg_to_fleets.items() if f_set == all_fleet_names)

    # Build layers
    base = Layer(name="base", parent=None, packages=base_packages, fleets=fleet_names)

    derived_layers = []
    for f in fleets:
        derived_packages = sorted(pkg for pkg in f.packages if pkg not in base_packages)
        derived = Layer(
            name=f.name,
            parent="base",
            packages=derived_packages,
            configs=list(f.configs),
            fleets=[f.name],
        )
        derived_layers.append(derived)

    layers = [base] + derived_layers
    topo = LayerTopology(layers=layers, fleets=fleet_infos)
    topo._recalc_all()
    return topo
