package agentmeta

import "testing"

func TestAllAgentsAreStableAndReplyable(t *testing.T) {
	all := All()
	if len(all) != 4 {
		t.Fatalf("All() length = %d, want 4", len(all))
	}
	want := []string{OpenCode, Codex, Antigravity, Devin}
	for index, descriptor := range all {
		if descriptor.ID != want[index] {
			t.Fatalf("All()[%d].ID = %q, want %q", index, descriptor.ID, want[index])
		}
		if descriptor.DisplayName == "" || descriptor.TitlePrefix == "" || descriptor.FooterLabel == "" {
			t.Fatalf("incomplete descriptor: %#v", descriptor)
		}
		if !descriptor.Replyable {
			t.Fatalf("%s should be replyable", descriptor.ID)
		}
	}
}

func TestLookupNormalizesAgentID(t *testing.T) {
	descriptor, ok := Lookup("  ANTIGRAVITY ")
	if !ok || descriptor.ID != Antigravity || descriptor.DisplayName != "Antigravity" {
		t.Fatalf("Lookup = %#v, %v", descriptor, ok)
	}
	if _, ok := Lookup("unknown"); ok {
		t.Fatal("unknown agent was accepted")
	}
}
