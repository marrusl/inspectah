// Package architect provides cross-fleet layer topology decomposition.
//
// It analyzes multiple fleet merge outputs to identify shared (base) and
// role-specific (derived) package layers, producing a LayerTopology that
// can be exported as Containerfiles.
package architect

import (
	"fmt"
	"math"
	"sort"
)

// FleetInput is simplified fleet data ready for analysis.
type FleetInput struct {
	Name                   string
	Packages               []string
	Configs                []string
	HostCount              int
	BaseImage              string
	UnavailablePackages    []string
	DirectInstallPackages  []string
	UnverifiablePackages   []string
	PreflightStatus        string
}

// FleetInfo holds fleet metadata for the topology.
type FleetInfo struct {
	Name          string `json:"name"`
	HostCount     int    `json:"host_count"`
	TotalPackages int    `json:"total_packages"`
}

// Layer represents a layer in the image topology.
type Layer struct {
	Name       string   `json:"name"`
	Parent     *string  `json:"parent"`
	Packages   []string `json:"packages"`
	Configs    []string `json:"configs"`
	Fleets     []string `json:"fleets"`
	FanOut     int      `json:"fan_out"`
	Turbulence float64  `json:"turbulence"`
}

// recalcTurbulence recomputes the turbulence score for a layer.
func (l *Layer) recalcTurbulence() {
	raw := float64(l.FanOut) * (float64(len(l.Packages)) / 50.0)
	if l.Parent != nil { // non-base layers get floor of 1.0
		l.Turbulence = math.Max(1.0, raw)
	} else {
		l.Turbulence = raw
	}
}

// LayerTopology is the complete layer topology with manipulation support.
type LayerTopology struct {
	Layers []Layer     `json:"layers"`
	Fleets []FleetInfo `json:"fleets"`
}

// GetLayer returns the layer with the given name, or nil if not found.
func (t *LayerTopology) GetLayer(name string) *Layer {
	for i := range t.Layers {
		if t.Layers[i].Name == name {
			return &t.Layers[i]
		}
	}
	return nil
}

// MovePackage moves a package between layers.
//
// Standard move: remove from src, add to dst.
// Special case -- moving FROM base: package is broadcast to ALL derived
// layers (every fleet still needs it). The toLayer parameter identifies
// the user's chosen target; broadcast is a side effect.
func (t *LayerTopology) MovePackage(pkg, fromLayer, toLayer string) error {
	src := t.GetLayer(fromLayer)
	if src == nil {
		return fmt.Errorf("layer %q not found", fromLayer)
	}
	dst := t.GetLayer(toLayer)
	if dst == nil {
		return fmt.Errorf("layer %q not found", toLayer)
	}
	if !containsString(src.Packages, pkg) {
		return fmt.Errorf("package %q not found in layer %q", pkg, fromLayer)
	}

	src.Packages = removeString(src.Packages, pkg)

	if fromLayer == "base" {
		// Broadcast to ALL derived layers
		for i := range t.Layers {
			if t.Layers[i].Parent != nil && !containsString(t.Layers[i].Packages, pkg) {
				t.Layers[i].Packages = append(t.Layers[i].Packages, pkg)
			}
		}
	} else {
		if !containsString(dst.Packages, pkg) {
			dst.Packages = append(dst.Packages, pkg)
		}
	}

	t.recalcAll()
	return nil
}

// CopyPackage copies a package to another layer without removing from source.
func (t *LayerTopology) CopyPackage(pkg, fromLayer, toLayer string) error {
	src := t.GetLayer(fromLayer)
	if src == nil {
		return fmt.Errorf("layer %q not found", fromLayer)
	}
	dst := t.GetLayer(toLayer)
	if dst == nil {
		return fmt.Errorf("layer %q not found", toLayer)
	}
	if !containsString(src.Packages, pkg) {
		return fmt.Errorf("package %q not found in layer %q", pkg, fromLayer)
	}
	if !containsString(dst.Packages, pkg) {
		dst.Packages = append(dst.Packages, pkg)
	}
	t.recalcAll()
	return nil
}

// ToDict returns the topology as a serializable map.
func (t *LayerTopology) ToDict() map[string]interface{} {
	layers := make([]map[string]interface{}, len(t.Layers))
	for i, l := range t.Layers {
		layers[i] = map[string]interface{}{
			"name":       l.Name,
			"parent":     l.Parent,
			"packages":   l.Packages,
			"configs":    l.Configs,
			"fleets":     l.Fleets,
			"fan_out":    l.FanOut,
			"turbulence": math.Round(l.Turbulence*10) / 10,
		}
	}
	fleets := make([]map[string]interface{}, len(t.Fleets))
	for i, f := range t.Fleets {
		fleets[i] = map[string]interface{}{
			"name":           f.Name,
			"host_count":     f.HostCount,
			"total_packages": f.TotalPackages,
		}
	}
	return map[string]interface{}{
		"layers": layers,
		"fleets": fleets,
	}
}

// recalcAll recomputes fan_out and turbulence for all layers.
func (t *LayerTopology) recalcAll() {
	base := t.GetLayer("base")
	for i := range t.Layers {
		l := &t.Layers[i]
		if base != nil && l.Parent == nil && l.Name == "base" {
			// Base fan_out = number of derived layers
			l.FanOut = 0
			for _, other := range t.Layers {
				if other.Parent != nil {
					l.FanOut++
				}
			}
		} else if l.Parent != nil {
			l.FanOut = 1
		}
		l.recalcTurbulence()
	}
}

// AnalyzeFleets analyzes multiple fleets and produces a layer topology.
//
// Uses 100% cross-fleet prevalence heuristic: packages in ALL fleets go
// to base. Remaining packages stay in their fleet's derived layer.
// Configs always stay with their original fleet (not decomposed).
// Single fleet -> no base extraction (fleet becomes the only layer).
func AnalyzeFleets(fleets []FleetInput) *LayerTopology {
	fleetInfos := make([]FleetInfo, len(fleets))
	for i, f := range fleets {
		fleetInfos[i] = FleetInfo{
			Name:          f.Name,
			HostCount:     f.HostCount,
			TotalPackages: len(f.Packages),
		}
	}

	if len(fleets) == 1 {
		f := fleets[0]
		layer := Layer{
			Name:     f.Name,
			Parent:   nil,
			Packages: copyStrings(f.Packages),
			Configs:  copyStrings(f.Configs),
			Fleets:   []string{f.Name},
		}
		layer.recalcTurbulence()
		topo := &LayerTopology{
			Layers: []Layer{layer},
			Fleets: fleetInfos,
		}
		return topo
	}

	// Build cross-fleet package index
	fleetNames := make([]string, len(fleets))
	for i, f := range fleets {
		fleetNames[i] = f.Name
	}
	allFleetSet := make(map[string]bool, len(fleetNames))
	for _, n := range fleetNames {
		allFleetSet[n] = true
	}

	pkgToFleets := make(map[string]map[string]bool)
	for _, f := range fleets {
		for _, pkg := range f.Packages {
			if pkgToFleets[pkg] == nil {
				pkgToFleets[pkg] = make(map[string]bool)
			}
			pkgToFleets[pkg][f.Name] = true
		}
	}

	// Packages in ALL fleets -> base
	var basePackages []string
	for pkg, fSet := range pkgToFleets {
		if len(fSet) == len(allFleetSet) {
			allMatch := true
			for n := range allFleetSet {
				if !fSet[n] {
					allMatch = false
					break
				}
			}
			if allMatch {
				basePackages = append(basePackages, pkg)
			}
		}
	}
	sort.Strings(basePackages)

	basePkgSet := make(map[string]bool, len(basePackages))
	for _, p := range basePackages {
		basePkgSet[p] = true
	}

	// Build layers
	base := Layer{
		Name:     "base",
		Parent:   nil,
		Packages: basePackages,
		Configs:  nil,
		Fleets:   copyStrings(fleetNames),
	}

	derivedLayers := make([]Layer, len(fleets))
	parentName := "base"
	for i, f := range fleets {
		var derivedPkgs []string
		for _, pkg := range f.Packages {
			if !basePkgSet[pkg] {
				derivedPkgs = append(derivedPkgs, pkg)
			}
		}
		sort.Strings(derivedPkgs)
		derivedLayers[i] = Layer{
			Name:     f.Name,
			Parent:   &parentName,
			Packages: derivedPkgs,
			Configs:  copyStrings(f.Configs),
			Fleets:   []string{f.Name},
		}
	}

	layers := make([]Layer, 0, 1+len(derivedLayers))
	layers = append(layers, base)
	layers = append(layers, derivedLayers...)

	topo := &LayerTopology{
		Layers: layers,
		Fleets: fleetInfos,
	}
	topo.recalcAll()
	return topo
}

// --- helpers ---

func containsString(ss []string, s string) bool {
	for _, v := range ss {
		if v == s {
			return true
		}
	}
	return false
}

func removeString(ss []string, s string) []string {
	result := make([]string, 0, len(ss))
	for _, v := range ss {
		if v != s {
			result = append(result, v)
		}
	}
	return result
}

func copyStrings(ss []string) []string {
	if ss == nil {
		return nil
	}
	out := make([]string, len(ss))
	copy(out, ss)
	return out
}
