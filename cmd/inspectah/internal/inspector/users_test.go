package inspector

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

func loadUsersFixture(t *testing.T, name string) string {
	t.Helper()
	data, err := os.ReadFile(filepath.Join("testdata", "users", name))
	require.NoError(t, err, "loading fixture %s", name)
	return string(data)
}

// ---------------------------------------------------------------------------
// parsePasswd
// ---------------------------------------------------------------------------

func TestParsePasswd(t *testing.T) {
	tests := []struct {
		name       string
		input      string
		wantUsers  int
		wantNames  []string
		wantUIDs   []int
	}{
		{
			name:      "typical RHEL passwd",
			input:     loadUsersFixture(t, "passwd"),
			wantUsers: 4,
			wantNames: []string{"alice", "bob", "svcaccount", "webapp"},
			wantUIDs:  []int{1000, 1001, 1002, 1003},
		},
		{
			name:      "empty file",
			input:     "",
			wantUsers: 0,
		},
		{
			name:      "only system users",
			input:     "root:x:0:0:root:/root:/bin/bash\nsshd:x:74:74:sshd:/usr/share/empty.sshd:/sbin/nologin\n",
			wantUsers: 0,
		},
		{
			name:      "skip comments",
			input:     "# this is a comment\nalice:x:1000:1000:Alice:/home/alice:/bin/bash\n",
			wantUsers: 1,
			wantNames: []string{"alice"},
		},
		{
			name:      "malformed line",
			input:     "alice:x:notanumber:1000:Alice:/home/alice:/bin/bash\nbob:x:1001:1001:Bob:/home/bob:/bin/bash\n",
			wantUsers: 1,
			wantNames: []string{"bob"},
		},
		{
			name:      "short line",
			input:     "alice:x:1000\n",
			wantUsers: 0,
		},
		{
			name:      "UID at upper boundary excluded",
			input:     "biguid:x:60000:60000:Big:/home/big:/bin/bash\n",
			wantUsers: 0,
		},
		{
			name:      "UID at lower boundary included",
			input:     "minuid:x:1000:1000:Min:/home/min:/bin/bash\n",
			wantUsers: 1,
			wantNames: []string{"minuid"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			section := newEmptyUserGroupSection()
			nonSystem := map[string]bool{}
			parsePasswd(tt.input, section, nonSystem)

			assert.Len(t, section.Users, tt.wantUsers)
			assert.Len(t, section.PasswdEntries, tt.wantUsers)
			assert.Len(t, nonSystem, tt.wantUsers)

			for i, name := range tt.wantNames {
				assert.Equal(t, name, section.Users[i]["name"])
			}
			for i, uid := range tt.wantUIDs {
				assert.Equal(t, uid, section.Users[i]["uid"])
			}
		})
	}
}

func TestParsePasswdFields(t *testing.T) {
	input := "alice:x:1000:1000:Alice Smith:/home/alice:/bin/bash\n"
	section := newEmptyUserGroupSection()
	nonSystem := map[string]bool{}
	parsePasswd(input, section, nonSystem)

	require.Len(t, section.Users, 1)
	u := section.Users[0]
	assert.Equal(t, "alice", u["name"])
	assert.Equal(t, 1000, u["uid"])
	assert.Equal(t, 1000, u["gid"])
	assert.Equal(t, "/bin/bash", u["shell"])
	assert.Equal(t, "/home/alice", u["home"])
	assert.Equal(t, true, u["include"])
}

func TestParsePasswdNonNumericGID(t *testing.T) {
	input := "alice:x:1000:badgid:Alice:/home/alice:/bin/bash\n"
	section := newEmptyUserGroupSection()
	nonSystem := map[string]bool{}
	parsePasswd(input, section, nonSystem)

	require.Len(t, section.Users, 1)
	assert.Nil(t, section.Users[0]["gid"])
}

// ---------------------------------------------------------------------------
// classifyUser
// ---------------------------------------------------------------------------

func TestClassifyUser(t *testing.T) {
	tests := []struct {
		name string
		user map[string]interface{}
		want string
	}{
		{
			name: "human user",
			user: map[string]interface{}{"shell": "/bin/bash", "home": "/home/alice", "uid": 1000},
			want: "human",
		},
		{
			name: "service nologin",
			user: map[string]interface{}{"shell": "/sbin/nologin", "home": "/var/lib/svc", "uid": 1002},
			want: "service",
		},
		{
			name: "service /bin/false",
			user: map[string]interface{}{"shell": "/bin/false", "home": "/home/locked", "uid": 1005},
			want: "service",
		},
		{
			name: "service devnull home",
			user: map[string]interface{}{"shell": "/bin/bash", "home": "/dev/null", "uid": 1003},
			want: "service",
		},
		{
			name: "service empty home",
			user: map[string]interface{}{"shell": "/bin/bash", "home": "", "uid": 1003},
			want: "service",
		},
		{
			name: "ambiguous /opt with real shell",
			user: map[string]interface{}{"shell": "/bin/bash", "home": "/opt/webapp", "uid": 1003},
			want: "ambiguous",
		},
		{
			name: "service /var without real shell",
			user: map[string]interface{}{"shell": "/usr/sbin/nologin", "home": "/var/lib/app", "uid": 1003},
			want: "service",
		},
		{
			name: "service /srv without real shell",
			user: map[string]interface{}{"shell": "/some/custom/shell", "home": "/srv/data", "uid": 1003},
			want: "service",
		},
		{
			name: "ambiguous /srv with real shell",
			user: map[string]interface{}{"shell": "/usr/bin/zsh", "home": "/srv/data", "uid": 1003},
			want: "ambiguous",
		},
		{
			name: "ambiguous unknown shell",
			user: map[string]interface{}{"shell": "/usr/local/bin/custom", "home": "/home/strange", "uid": 1004},
			want: "ambiguous",
		},
		{
			name: "human with zsh",
			user: map[string]interface{}{"shell": "/bin/zsh", "home": "/home/bob", "uid": 1001},
			want: "human",
		},
		{
			name: "human with fish",
			user: map[string]interface{}{"shell": "/usr/bin/fish", "home": "/home/dev", "uid": 1005},
			want: "human",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := classifyUser(tt.user)
			assert.Equal(t, tt.want, got)
		})
	}
}

// ---------------------------------------------------------------------------
// parseShadow
// ---------------------------------------------------------------------------

func TestParseShadow(t *testing.T) {
	tests := []struct {
		name    string
		input   string
		users   map[string]bool
		wantLen int
	}{
		{
			name:    "matches non-system users only",
			input:   loadUsersFixture(t, "shadow"),
			users:   map[string]bool{"alice": true, "bob": true, "svcaccount": true, "webapp": true},
			wantLen: 4,
		},
		{
			name:    "no matching users",
			input:   "root:$6$abc::0:99999:7:::\n",
			users:   map[string]bool{"alice": true},
			wantLen: 0,
		},
		{
			name:    "empty file",
			input:   "",
			users:   map[string]bool{"alice": true},
			wantLen: 0,
		},
		{
			name:    "skip comments",
			input:   "# comment\nalice:$6$abc:19500:0:99999:7:::\n",
			users:   map[string]bool{"alice": true},
			wantLen: 1,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			section := newEmptyUserGroupSection()
			parseShadow(tt.input, section, tt.users)
			assert.Len(t, section.ShadowEntries, tt.wantLen)
		})
	}
}

// ---------------------------------------------------------------------------
// parseGroup
// ---------------------------------------------------------------------------

func TestParseGroup(t *testing.T) {
	tests := []struct {
		name       string
		input      string
		wantGroups int
		wantNames  []string
	}{
		{
			name:       "typical RHEL group file",
			input:      loadUsersFixture(t, "group"),
			wantGroups: 5,
			wantNames:  []string{"alice", "bob", "svcaccount", "webapp", "devteam"},
		},
		{
			name:       "empty file",
			input:      "",
			wantGroups: 0,
		},
		{
			name:       "only system groups",
			input:      "root:x:0:\nwheel:x:10:alice\n",
			wantGroups: 0,
		},
		{
			name:       "group with members",
			input:      "devteam:x:1004:alice,bob,webapp\n",
			wantGroups: 1,
			wantNames:  []string{"devteam"},
		},
		{
			name:       "GID at upper boundary excluded",
			input:      "biggid:x:60000:\n",
			wantGroups: 0,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			section := newEmptyUserGroupSection()
			nonSystemGroups := map[string]bool{}
			parseGroup(tt.input, section, nonSystemGroups)

			assert.Len(t, section.Groups, tt.wantGroups)
			assert.Len(t, section.GroupEntries, tt.wantGroups)

			for i, name := range tt.wantNames {
				assert.Equal(t, name, section.Groups[i]["name"])
			}
		})
	}
}

func TestParseGroupMembers(t *testing.T) {
	input := "devteam:x:1004:alice,bob,webapp\n"
	section := newEmptyUserGroupSection()
	nonSystemGroups := map[string]bool{}
	parseGroup(input, section, nonSystemGroups)

	require.Len(t, section.Groups, 1)
	members := section.Groups[0]["members"].([]interface{})
	assert.Equal(t, []interface{}{"alice", "bob", "webapp"}, members)
}

func TestParseGroupEmptyMembers(t *testing.T) {
	input := "alice:x:1000:\n"
	section := newEmptyUserGroupSection()
	nonSystemGroups := map[string]bool{}
	parseGroup(input, section, nonSystemGroups)

	require.Len(t, section.Groups, 1)
	members := section.Groups[0]["members"].([]interface{})
	assert.Empty(t, members)
}

// ---------------------------------------------------------------------------
// parseGshadow
// ---------------------------------------------------------------------------

func TestParseGshadow(t *testing.T) {
	input := loadUsersFixture(t, "gshadow")
	section := newEmptyUserGroupSection()
	nonSystemGroups := map[string]bool{"alice": true, "bob": true, "devteam": true}
	parseGshadow(input, section, nonSystemGroups)

	assert.Len(t, section.GshadowEntries, 3)
}

// ---------------------------------------------------------------------------
// extractSudoersRules
// ---------------------------------------------------------------------------

func TestExtractSudoersRules(t *testing.T) {
	tests := []struct {
		name     string
		input    string
		wantLen  int
		wantRule string
	}{
		{
			name:     "typical sudoers",
			input:    loadUsersFixture(t, "sudoers"),
			wantLen:  5,
			wantRule: "alice ALL=(ALL) NOPASSWD: ALL",
		},
		{
			name:    "empty file",
			input:   "",
			wantLen: 0,
		},
		{
			name:    "only comments and defaults",
			input:   "# comment\nDefaults env_reset\n",
			wantLen: 0,
		},
		{
			name:     "sudoers.d drop-in",
			input:    loadUsersFixture(t, "sudoers.d-webapp"),
			wantLen:  1,
			wantRule: "webapp ALL=(ALL) NOPASSWD: /usr/bin/systemctl restart webapp",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			section := newEmptyUserGroupSection()
			extractSudoersRules(tt.input, section)
			assert.Len(t, section.SudoersRules, tt.wantLen)
			if tt.wantRule != "" {
				assert.Contains(t, section.SudoersRules, tt.wantRule)
			}
		})
	}
}

// ---------------------------------------------------------------------------
// assignGroupStrategies
// ---------------------------------------------------------------------------

func TestAssignGroupStrategies(t *testing.T) {
	section := newEmptyUserGroupSection()
	section.Users = []map[string]interface{}{
		{"name": "alice", "gid": 1000, "strategy": "kickstart"},
		{"name": "svc", "gid": 1002, "strategy": "sysusers"},
	}
	section.Groups = []map[string]interface{}{
		{"name": "alice", "gid": 1000},
		{"name": "svc", "gid": 1002},
		{"name": "devteam", "gid": 1004},
	}

	assignGroupStrategies(section, "")

	assert.Equal(t, "kickstart", section.Groups[0]["strategy"])
	assert.Equal(t, "sysusers", section.Groups[1]["strategy"])
	assert.Equal(t, "sysusers", section.Groups[2]["strategy"]) // no primary user
}

func TestAssignGroupStrategiesOverride(t *testing.T) {
	section := newEmptyUserGroupSection()
	section.Users = []map[string]interface{}{
		{"name": "alice", "gid": 1000, "strategy": "kickstart"},
	}
	section.Groups = []map[string]interface{}{
		{"name": "alice", "gid": 1000},
	}

	assignGroupStrategies(section, "useradd")

	assert.Equal(t, "useradd", section.Groups[0]["strategy"])
}

// ---------------------------------------------------------------------------
// Full integration test via FakeExecutor
// ---------------------------------------------------------------------------

func TestRunUsersGroups_Integration(t *testing.T) {
	fake := NewFakeExecutor(nil).WithFiles(map[string]string{
		"/etc/passwd":  loadUsersFixture(t, "passwd"),
		"/etc/shadow":  loadUsersFixture(t, "shadow"),
		"/etc/group":   loadUsersFixture(t, "group"),
		"/etc/gshadow": loadUsersFixture(t, "gshadow"),
		"/etc/subuid":  loadUsersFixture(t, "subuid"),
		"/etc/subgid":  loadUsersFixture(t, "subgid"),
		"/etc/sudoers": loadUsersFixture(t, "sudoers"),
		"/home/alice/.ssh/authorized_keys": loadUsersFixture(t, "authorized_keys"),
	})

	section, warnings, err := RunUsersGroups(fake, UserGroupOptions{})
	require.NoError(t, err)
	assert.Empty(t, warnings)

	// Users: alice, bob, svcaccount, webapp
	assert.Len(t, section.Users, 4)
	assert.Len(t, section.PasswdEntries, 4)

	// Verify classification
	userMap := map[string]map[string]interface{}{}
	for _, u := range section.Users {
		userMap[u["name"].(string)] = u
	}
	assert.Equal(t, "human", userMap["alice"]["classification"])
	assert.Equal(t, "human", userMap["bob"]["classification"])
	assert.Equal(t, "service", userMap["svcaccount"]["classification"])
	assert.Equal(t, "ambiguous", userMap["webapp"]["classification"])

	// Verify strategies
	assert.Equal(t, "kickstart", userMap["alice"]["strategy"])
	assert.Equal(t, "kickstart", userMap["bob"]["strategy"])
	assert.Equal(t, "sysusers", userMap["svcaccount"]["strategy"])
	assert.Equal(t, "useradd", userMap["webapp"]["strategy"])

	// Shadow: alice, bob, svcaccount, webapp
	assert.Len(t, section.ShadowEntries, 4)

	// Groups: alice, bob, svcaccount, webapp, devteam
	assert.Len(t, section.Groups, 5)
	assert.Len(t, section.GroupEntries, 5)

	// Gshadow: alice, bob, devteam (svcaccount and webapp not in nonSystemGroups fixture)
	// All 5 groups are non-system, so all gshadow entries matching those group names
	assert.Len(t, section.GshadowEntries, 5)

	// Subuid/subgid: alice, bob
	assert.Len(t, section.SubuidEntries, 2)
	assert.Len(t, section.SubgidEntries, 2)

	// Sudoers: rules from /etc/sudoers (no sudoers.d in this test)
	assert.GreaterOrEqual(t, len(section.SudoersRules), 4)

	// SSH keys: alice has authorized_keys
	assert.Len(t, section.SSHAuthorizedKeysRefs, 1)
	assert.Equal(t, "alice", section.SSHAuthorizedKeysRefs[0]["user"])
	assert.Equal(t, "/home/alice/.ssh/authorized_keys", section.SSHAuthorizedKeysRefs[0]["path"])
}

func TestRunUsersGroups_StrategyOverride(t *testing.T) {
	fake := NewFakeExecutor(nil).WithFiles(map[string]string{
		"/etc/passwd": "alice:x:1000:1000:Alice:/home/alice:/bin/bash\nsvc:x:1001:1001:Svc:/var/lib/svc:/sbin/nologin\n",
		"/etc/group":  "alice:x:1000:\nsvc:x:1001:\n",
	})

	section, _, err := RunUsersGroups(fake, UserGroupOptions{UserStrategyOverride: "kickstart"})
	require.NoError(t, err)

	for _, u := range section.Users {
		assert.Equal(t, "kickstart", u["strategy"])
	}
	for _, g := range section.Groups {
		assert.Equal(t, "kickstart", g["strategy"])
	}
}

func TestRunUsersGroups_EmptySystem(t *testing.T) {
	fake := NewFakeExecutor(nil)

	section, warnings, err := RunUsersGroups(fake, UserGroupOptions{})
	require.NoError(t, err)
	assert.Empty(t, warnings)
	assert.Empty(t, section.Users)
	assert.Empty(t, section.Groups)
	assert.Empty(t, section.SudoersRules)
	assert.Empty(t, section.SSHAuthorizedKeysRefs)
	assert.Empty(t, section.PasswdEntries)
	assert.Empty(t, section.ShadowEntries)
	assert.Empty(t, section.GroupEntries)
	assert.Empty(t, section.GshadowEntries)
	assert.Empty(t, section.SubuidEntries)
	assert.Empty(t, section.SubgidEntries)
}

func TestRunUsersGroups_SudoersD(t *testing.T) {
	fake := NewFakeExecutor(nil).
		WithFiles(map[string]string{
			"/etc/passwd":            "webapp:x:1003:1003:Web:/opt/webapp:/bin/bash\n",
			"/etc/sudoers":           "root\tALL=(ALL)\tALL\n",
			"/etc/sudoers.d/webapp":  "webapp ALL=(ALL) NOPASSWD: /usr/bin/systemctl restart webapp\n",
			"/etc/sudoers.d/.hidden": "# this should be skipped\nhidden ALL=(ALL) ALL\n",
		}).
		WithDirs(map[string][]string{
			"/etc/sudoers.d": {"webapp", ".hidden"},
		})

	section, _, err := RunUsersGroups(fake, UserGroupOptions{})
	require.NoError(t, err)

	// root ALL from main sudoers + webapp from drop-in; .hidden skipped
	assert.Contains(t, section.SudoersRules, "root\tALL=(ALL)\tALL")
	assert.Contains(t, section.SudoersRules, "webapp ALL=(ALL) NOPASSWD: /usr/bin/systemctl restart webapp")

	// The .hidden file is skipped
	for _, rule := range section.SudoersRules {
		assert.NotContains(t, rule, "hidden ALL")
	}
}

func TestRunUsersGroups_NoSSHKeys(t *testing.T) {
	fake := NewFakeExecutor(nil).WithFiles(map[string]string{
		"/etc/passwd": "alice:x:1000:1000:Alice:/home/alice:/bin/bash\n",
	})

	section, _, err := RunUsersGroups(fake, UserGroupOptions{})
	require.NoError(t, err)
	assert.Empty(t, section.SSHAuthorizedKeysRefs)
}

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

func newEmptyUserGroupSection() *schema.UserGroupSection {
	return &schema.UserGroupSection{
		Users:                 []map[string]interface{}{},
		Groups:                []map[string]interface{}{},
		SudoersRules:          []string{},
		SSHAuthorizedKeysRefs: []map[string]interface{}{},
		PasswdEntries:         []string{},
		ShadowEntries:         []string{},
		GroupEntries:          []string{},
		GshadowEntries:        []string{},
		SubuidEntries:         []string{},
		SubgidEntries:         []string{},
	}
}
