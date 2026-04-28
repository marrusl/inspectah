package architect

import (
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestAnalyzeFleets_SingleFleet(t *testing.T) {
	fleets := []FleetInput{
		{
			Name:      "web",
			Packages:  []string{"httpd", "mod_ssl", "php"},
			Configs:   []string{"/etc/httpd/httpd.conf"},
			HostCount: 3,
		},
	}

	topo := AnalyzeFleets(fleets)

	require.Len(t, topo.Layers, 1)
	assert.Equal(t, "web", topo.Layers[0].Name)
	assert.Nil(t, topo.Layers[0].Parent)
	assert.ElementsMatch(t, []string{"httpd", "mod_ssl", "php"}, topo.Layers[0].Packages)
	assert.Equal(t, []string{"/etc/httpd/httpd.conf"}, topo.Layers[0].Configs)
	assert.Equal(t, []string{"web"}, topo.Layers[0].Fleets)

	require.Len(t, topo.Fleets, 1)
	assert.Equal(t, "web", topo.Fleets[0].Name)
	assert.Equal(t, 3, topo.Fleets[0].HostCount)
	assert.Equal(t, 3, topo.Fleets[0].TotalPackages)
}

func TestAnalyzeFleets_MultipleFleets(t *testing.T) {
	fleets := []FleetInput{
		{
			Name:      "web",
			Packages:  []string{"httpd", "mod_ssl", "bash", "coreutils"},
			Configs:   []string{"/etc/httpd/httpd.conf"},
			HostCount: 5,
		},
		{
			Name:      "db",
			Packages:  []string{"postgresql", "bash", "coreutils"},
			Configs:   []string{"/etc/postgresql/postgresql.conf"},
			HostCount: 2,
		},
	}

	topo := AnalyzeFleets(fleets)

	// Should have 3 layers: base, web, db
	require.Len(t, topo.Layers, 3)

	// Base layer
	base := topo.GetLayer("base")
	require.NotNil(t, base)
	assert.Nil(t, base.Parent)
	assert.ElementsMatch(t, []string{"bash", "coreutils"}, base.Packages)
	assert.ElementsMatch(t, []string{"web", "db"}, base.Fleets)
	assert.Empty(t, base.Configs) // configs stay with their fleet

	// Web derived layer
	web := topo.GetLayer("web")
	require.NotNil(t, web)
	assert.NotNil(t, web.Parent)
	assert.Equal(t, "base", *web.Parent)
	assert.ElementsMatch(t, []string{"httpd", "mod_ssl"}, web.Packages)
	assert.Equal(t, []string{"/etc/httpd/httpd.conf"}, web.Configs)

	// DB derived layer
	db := topo.GetLayer("db")
	require.NotNil(t, db)
	assert.NotNil(t, db.Parent)
	assert.Equal(t, "base", *db.Parent)
	assert.ElementsMatch(t, []string{"postgresql"}, db.Packages)
	assert.Equal(t, []string{"/etc/postgresql/postgresql.conf"}, db.Configs)
}

func TestAnalyzeFleets_NoSharedPackages(t *testing.T) {
	fleets := []FleetInput{
		{Name: "web", Packages: []string{"httpd", "mod_ssl"}, HostCount: 2},
		{Name: "db", Packages: []string{"postgresql", "pgbouncer"}, HostCount: 1},
	}

	topo := AnalyzeFleets(fleets)
	require.Len(t, topo.Layers, 3)

	base := topo.GetLayer("base")
	require.NotNil(t, base)
	assert.Empty(t, base.Packages) // no shared packages
}

func TestAnalyzeFleets_AllSharedPackages(t *testing.T) {
	fleets := []FleetInput{
		{Name: "web", Packages: []string{"httpd", "bash"}, HostCount: 2},
		{Name: "db", Packages: []string{"httpd", "bash"}, HostCount: 1},
	}

	topo := AnalyzeFleets(fleets)
	require.Len(t, topo.Layers, 3)

	base := topo.GetLayer("base")
	require.NotNil(t, base)
	assert.ElementsMatch(t, []string{"bash", "httpd"}, base.Packages)

	// Derived layers should have no packages
	web := topo.GetLayer("web")
	require.NotNil(t, web)
	assert.Empty(t, web.Packages)

	db := topo.GetLayer("db")
	require.NotNil(t, db)
	assert.Empty(t, db.Packages)
}

func TestMovePackage_StandardMove(t *testing.T) {
	topo := makeTestTopology()

	err := topo.MovePackage("httpd", "web", "db")
	require.NoError(t, err)

	web := topo.GetLayer("web")
	assert.NotContains(t, web.Packages, "httpd")

	db := topo.GetLayer("db")
	assert.Contains(t, db.Packages, "httpd")
}

func TestMovePackage_FromBase_Broadcasts(t *testing.T) {
	topo := makeTestTopology()

	err := topo.MovePackage("bash", "base", "web")
	require.NoError(t, err)

	base := topo.GetLayer("base")
	assert.NotContains(t, base.Packages, "bash")

	// Both derived layers should get it
	web := topo.GetLayer("web")
	assert.Contains(t, web.Packages, "bash")

	db := topo.GetLayer("db")
	assert.Contains(t, db.Packages, "bash")
}

func TestMovePackage_ErrorCases(t *testing.T) {
	topo := makeTestTopology()

	err := topo.MovePackage("httpd", "nonexistent", "db")
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "not found")

	err = topo.MovePackage("httpd", "web", "nonexistent")
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "not found")

	err = topo.MovePackage("nonexistent-pkg", "web", "db")
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "not found")
}

func TestCopyPackage(t *testing.T) {
	topo := makeTestTopology()

	err := topo.CopyPackage("httpd", "web", "db")
	require.NoError(t, err)

	// Source still has it
	web := topo.GetLayer("web")
	assert.Contains(t, web.Packages, "httpd")

	// Destination also has it
	db := topo.GetLayer("db")
	assert.Contains(t, db.Packages, "httpd")
}

func TestCopyPackage_NoDuplication(t *testing.T) {
	topo := makeTestTopology()

	// Copy bash from base to web -- web shouldn't get duplicate
	// First add bash to web manually
	web := topo.GetLayer("web")
	web.Packages = append(web.Packages, "bash")

	err := topo.CopyPackage("bash", "base", "web")
	require.NoError(t, err)

	// Should not have duplicate
	count := 0
	for _, p := range web.Packages {
		if p == "bash" {
			count++
		}
	}
	assert.Equal(t, 1, count)
}

func TestGetLayer_NotFound(t *testing.T) {
	topo := makeTestTopology()
	assert.Nil(t, topo.GetLayer("nonexistent"))
}

func TestToDict(t *testing.T) {
	topo := makeTestTopology()
	d := topo.ToDict()

	layers, ok := d["layers"].([]map[string]interface{})
	require.True(t, ok)
	assert.Len(t, layers, 3)

	fleets, ok := d["fleets"].([]map[string]interface{})
	require.True(t, ok)
	assert.Len(t, fleets, 2)

	// Check base layer in dict
	assert.Equal(t, "base", layers[0]["name"])
	assert.Nil(t, layers[0]["parent"])
}

func TestTurbulence(t *testing.T) {
	topo := makeTestTopology()
	topo.recalcAll()

	base := topo.GetLayer("base")
	// base: fan_out=2, packages=2 -> 2 * (2/50) = 0.08
	assert.InDelta(t, 0.08, base.Turbulence, 0.01)

	web := topo.GetLayer("web")
	// derived: fan_out=1, packages=2 -> max(1.0, 1*(2/50)) = 1.0
	assert.InDelta(t, 1.0, web.Turbulence, 0.01)
}

func TestFleetInfo(t *testing.T) {
	fleets := []FleetInput{
		{Name: "web", Packages: []string{"httpd", "mod_ssl"}, HostCount: 5},
		{Name: "db", Packages: []string{"postgresql"}, HostCount: 2},
	}

	topo := AnalyzeFleets(fleets)

	require.Len(t, topo.Fleets, 2)
	assert.Equal(t, "web", topo.Fleets[0].Name)
	assert.Equal(t, 5, topo.Fleets[0].HostCount)
	assert.Equal(t, 2, topo.Fleets[0].TotalPackages)
	assert.Equal(t, "db", topo.Fleets[1].Name)
	assert.Equal(t, 2, topo.Fleets[1].HostCount)
	assert.Equal(t, 1, topo.Fleets[1].TotalPackages)
}

func TestThreeFleets(t *testing.T) {
	fleets := []FleetInput{
		{Name: "web", Packages: []string{"httpd", "bash", "coreutils"}, HostCount: 3},
		{Name: "db", Packages: []string{"postgresql", "bash", "coreutils"}, HostCount: 2},
		{Name: "cache", Packages: []string{"redis", "bash", "coreutils"}, HostCount: 1},
	}

	topo := AnalyzeFleets(fleets)
	require.Len(t, topo.Layers, 4)

	base := topo.GetLayer("base")
	require.NotNil(t, base)
	assert.ElementsMatch(t, []string{"bash", "coreutils"}, base.Packages)
	assert.Equal(t, 3, base.FanOut)
}

// --- test helpers ---

func makeTestTopology() *LayerTopology {
	parentName := "base"
	topo := &LayerTopology{
		Layers: []Layer{
			{
				Name:     "base",
				Parent:   nil,
				Packages: []string{"bash", "coreutils"},
				Fleets:   []string{"web", "db"},
			},
			{
				Name:     "web",
				Parent:   &parentName,
				Packages: []string{"httpd", "mod_ssl"},
				Configs:  []string{"/etc/httpd/httpd.conf"},
				Fleets:   []string{"web"},
			},
			{
				Name:     "db",
				Parent:   &parentName,
				Packages: []string{"postgresql"},
				Configs:  []string{"/etc/postgresql/postgresql.conf"},
				Fleets:   []string{"db"},
			},
		},
		Fleets: []FleetInfo{
			{Name: "web", HostCount: 5, TotalPackages: 4},
			{Name: "db", HostCount: 2, TotalPackages: 3},
		},
	}
	topo.recalcAll()
	return topo
}
