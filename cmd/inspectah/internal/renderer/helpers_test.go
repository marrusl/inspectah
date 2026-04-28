package renderer

import (
	"testing"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

func TestSanitizeShellValue(t *testing.T) {
	tests := []struct {
		val  string
		safe bool
	}{
		{"httpd.service", true},
		{"my-timer.timer", true},
		{"foo;bar", false},
		{"$(cmd)", false},
		{"val`cmd`", false},
		{"a|b", false},
		{"ok_name-123", true},
		{"a\nb", false},
		{"normal", true},
	}

	for _, tt := range tests {
		t.Run(tt.val, func(t *testing.T) {
			got := sanitizeShellValue(tt.val, "test")
			if tt.safe && got == nil {
				t.Errorf("expected safe, got unsafe")
			}
			if !tt.safe && got != nil {
				t.Errorf("expected unsafe, got safe")
			}
		})
	}
}

func TestIsBootloaderKarg(t *testing.T) {
	tests := []struct {
		karg       string
		bootloader bool
	}{
		{"ro", true},
		{"quiet", true},
		{"root=/dev/sda1", true},
		{"rd.lvm.lv=vg/lv", true},
		{"BOOT_IMAGE=/vmlinuz", true},
		{"net.ifnames=0", false},
		{"selinux=1", false},
		{"custom_param", false},
	}

	for _, tt := range tests {
		t.Run(tt.karg, func(t *testing.T) {
			got := isBootloaderKarg(tt.karg)
			if got != tt.bootloader {
				t.Errorf("got %v, want %v", got, tt.bootloader)
			}
		})
	}
}

func TestOperatorKargs(t *testing.T) {
	cmdline := "ro root=/dev/sda1 quiet net.ifnames=0 selinux=1 BOOT_IMAGE=/vmlinuz"
	got := operatorKargs(cmdline)

	// Should only include operator kargs
	want := map[string]bool{
		"net.ifnames=0": true,
		"selinux=1":     true,
	}

	for _, k := range got {
		if !want[k] {
			t.Errorf("unexpected karg: %s", k)
		}
		delete(want, k)
	}
	for k := range want {
		t.Errorf("missing karg: %s", k)
	}
}

func TestBaseImageFromSnapshot(t *testing.T) {
	// Default when no base image set
	snap := schema.NewSnapshot()
	got := baseImageFromSnapshot(snap)
	if got != "registry.redhat.io/rhel9/rhel-bootc:9.4" {
		t.Errorf("default: got %q", got)
	}

	// With base image set
	base := "registry.redhat.io/rhel10/rhel-bootc:10.0"
	snap.Rpm = &schema.RpmSection{BaseImage: &base}
	got = baseImageFromSnapshot(snap)
	if got != base {
		t.Errorf("custom: got %q, want %q", got, base)
	}
}

func TestDhcpConnectionPaths(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Network = &schema.NetworkSection{
		Connections: []schema.NMConnection{
			{Path: "/etc/NetworkManager/system-connections/eth0.nmconnection", Method: "auto"},
			{Path: "/etc/NetworkManager/system-connections/eth1.nmconnection", Method: "manual"},
		},
	}

	paths := dhcpConnectionPaths(snap)
	if !paths["etc/NetworkManager/system-connections/eth0.nmconnection"] {
		t.Error("expected eth0 DHCP path")
	}
	if paths["etc/NetworkManager/system-connections/eth1.nmconnection"] {
		t.Error("manual connection should not be in DHCP paths")
	}
}

func TestSummariseDiff(t *testing.T) {
	diff := `--- a/file
+++ b/file
@@ -1,3 +1,3 @@
-key1=old_val
+key1=new_val
 key2=unchanged
-removed_key=val
+added_key=val2`

	got := summariseDiff(diff)
	if len(got) == 0 {
		t.Fatal("expected non-empty results")
	}

	// Should include a "key1: old_val → new_val" style entry
	found := false
	for _, s := range got {
		if s == "key1: old_val → new_val" {
			found = true
			break
		}
	}
	if !found {
		t.Errorf("expected key1 change summary in %v", got)
	}
}
