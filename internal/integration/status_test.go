package integration

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/srafyhucl-cpu/agent-notify/internal/agentmeta"
	"github.com/srafyhucl-cpu/agent-notify/internal/config"
)

func TestCheckAllReportsConnectedIntegrations(t *testing.T) {
	paths, binary := testPaths(t)
	now := time.Date(2026, 9, 15, 10, 0, 0, 0, time.UTC)

	writeFile(t, paths.CodexConfig, `notify = [ "`+slashPath(binary)+`", "codex", "turn-ended" ]`)
	writeFile(t, paths.PluginFile, `
const BAKED_BIN = "`+slashPath(binary)+`"
const event = "session.execution.succeeded"
`)
	writeHeartbeat(t, filepath.Join(paths.OpenCodeReplyDir, "heartbeats", "opencode.json"), now)
	writeFile(t, paths.DevinConfig, `{"hooks":{"Stop":[{"hooks":[{"command":"`+slashPath(binary)+` devin stop"}]}]}}`)
	for _, name := range []string{"package.json", "extension.js", "acp-bridge.js"} {
		writeFile(t, filepath.Join(paths.DevinExtensionDir, name), "ok")
	}
	writeHeartbeat(t, filepath.Join(paths.DevinReplyDir, "heartbeats", "devin.json"), now)
	writeFile(t, paths.AntigravityLauncher, "@echo off\n@rem agent-notify-antigravity-launcher\n\""+binary+"\" antigravity stop\n")
	writeFile(t, paths.AntigravityHooks, `{"agent-notify":{"Stop":[{"type":"command","command":".\\agent-notify-hook.cmd antigravity stop"}]}}`)
	writeFile(t, paths.CommandCodeModFile, `
// agent-notify-commandcode-mod
const BAKED_BIN = "`+slashPath(binary)+`"
`)
	writeHeartbeat(t, filepath.Join(paths.CommandCodeReplyDir, "heartbeats", "commandcode.json"), now)

	statuses := CheckAll(Options{Paths: paths, Executable: binary, Now: now})
	for _, status := range statuses {
		if status.State != StateConnected {
			t.Fatalf("%s state = %s, detail = %s", status.Agent, status.State, status.Detail)
		}
	}
}

func TestCheckOpenCodeDistinguishesPendingAndConnected(t *testing.T) {
	paths, binary := testPaths(t)
	now := time.Date(2026, 9, 15, 10, 0, 0, 0, time.UTC)
	writeFile(t, paths.PluginFile, `
const BAKED_BIN = "`+slashPath(binary)+`"
const event = "session.execution.succeeded"
`)

	status := findStatus(t, CheckAll(Options{Paths: paths, Executable: binary, Now: now}), agentmeta.OpenCode)
	if status.State != StatePendingRestart || !status.NeedsRestart {
		t.Fatalf("pending status = %#v", status)
	}

	writeHeartbeat(t, filepath.Join(paths.OpenCodeReplyDir, "heartbeats", "opencode.json"), now)
	status = findStatus(t, CheckAll(Options{Paths: paths, Executable: binary, Now: now}), agentmeta.OpenCode)
	if status.State != StateConnected {
		t.Fatalf("connected status = %#v", status)
	}
}

func TestCheckCodexIdentifiesSafeRepairAndCustomNotify(t *testing.T) {
	paths, binary := testPaths(t)
	writeFile(t, paths.CodexConfig, `notify = [ "C:/tools/codex-computer-use.exe", "turn-ended" ]`)
	status := findStatus(t, CheckAll(Options{Paths: paths, Executable: binary}), agentmeta.Codex)
	if status.State != StateError || status.Repair != RepairCodexWatch || !status.Fixable {
		t.Fatalf("upstream status = %#v", status)
	}

	writeFile(t, paths.CodexConfig, `notify = [ "C:/tools/custom-notify.exe", "turn-ended" ]`)
	status = findStatus(t, CheckAll(Options{Paths: paths, Executable: binary}), agentmeta.Codex)
	if status.State != StateNotDetected || !status.InUse || status.Fixable {
		t.Fatalf("custom status = %#v", status)
	}
}

func TestCheckCodexAcceptsAgentNotifyChainedByPreviousNotify(t *testing.T) {
	paths, binary := testPaths(t)
	writeFile(t, paths.CodexConfig, `notify = [ "C:/tools/codex-computer-use.exe", "turn-ended", "--previous-notify", "[\"`+slashPath(binary)+`\",\"codex\",\"turn-ended\"]" ]`)
	status := findStatus(t, CheckAll(Options{Paths: paths, Executable: binary}), agentmeta.Codex)
	if status.State != StateConnected || !status.InUse {
		t.Fatalf("chained previous notify status = %#v", status)
	}
	if !strings.Contains(status.Detail, "codex-computer-use") {
		t.Fatalf("chained detail = %q", status.Detail)
	}
}

func TestCheckCodexReportsMissingChainedAgentNotify(t *testing.T) {
	paths, binary := testPaths(t)
	writeFile(t, paths.CodexConfig, `notify = [ "C:/tools/codex-computer-use.exe", "turn-ended", "--previous-notify", "[\"C:/stale/agent-notify.exe\",\"codex\"]" ]`)
	status := findStatus(t, CheckAll(Options{Paths: paths, Executable: binary}), agentmeta.Codex)
	if status.State != StateError || status.Fixable || !strings.Contains(status.Detail, "链式转发") {
		t.Fatalf("missing chained target status = %#v", status)
	}
}

func TestCheckHeartbeatIgnoresStaleAndMalformedFiles(t *testing.T) {
	paths, binary := testPaths(t)
	now := time.Date(2026, 9, 15, 10, 0, 0, 0, time.UTC)
	writeFile(t, paths.PluginFile, `const BAKED_BIN = "`+slashPath(binary)+`"
const event = "session.execution.succeeded"`)
	writeFile(t, filepath.Join(paths.OpenCodeReplyDir, "heartbeats", "stale.json"), `{"timestamp":"2026-09-15T09:59:00Z"}`)
	status := findStatus(t, CheckAll(Options{Paths: paths, Executable: binary, Now: now}), agentmeta.OpenCode)
	if status.State != StatePendingRestart {
		t.Fatalf("stale status = %#v", status)
	}

	writeFile(t, filepath.Join(paths.OpenCodeReplyDir, "heartbeats", "broken.json"), `{`)
	status = findStatus(t, CheckAll(Options{Paths: paths, Executable: binary, Now: now}), agentmeta.OpenCode)
	if status.State != StateError {
		t.Fatalf("malformed status = %#v", status)
	}
}

func TestDisabledStatusOnlyChangesLabel(t *testing.T) {
	paths, binary := testPaths(t)
	status := findStatus(t, CheckAll(Options{
		Paths:      paths,
		Executable: binary,
		Enabled:    map[string]bool{agentmeta.Codex: false},
	}), agentmeta.Codex)
	if status.Enabled || status.Label() != "已暂停" {
		t.Fatalf("disabled status = %#v, label = %q", status, status.Label())
	}
}

func testPaths(t *testing.T) (config.Paths, string) {
	t.Helper()
	root := t.TempDir()
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", filepath.Join(root, "config"))
	t.Setenv("AGENT_NOTIFY_TEMP_DIR", filepath.Join(root, "temp"))
	t.Setenv("AGENT_NOTIFY_PLUGIN_FILE", filepath.Join(root, "opencode", "agent-notify.ts"))
	t.Setenv("AGENT_NOTIFY_CODEX_CONFIG", filepath.Join(root, "codex", "config.toml"))
	t.Setenv("AGENT_NOTIFY_OPENCODE_REPLY_DIR", filepath.Join(root, "opencode-reply"))
	t.Setenv("AGENT_NOTIFY_DEVIN_CONFIG", filepath.Join(root, "devin", "config.json"))
	t.Setenv("AGENT_NOTIFY_DEVIN_EXTENSION_DIR", filepath.Join(root, "devin-extension"))
	t.Setenv("AGENT_NOTIFY_DEVIN_REPLY_DIR", filepath.Join(root, "devin-reply"))
	t.Setenv("AGENT_NOTIFY_ANTIGRAVITY_HOOKS", filepath.Join(root, "antigravity", "hooks.json"))
	t.Setenv("AGENT_NOTIFY_ANTIGRAVITY_LAUNCHER", filepath.Join(root, "antigravity", "agent-notify-hook.cmd"))
	t.Setenv("AGENT_NOTIFY_COMMANDCODE_MOD_FILE", filepath.Join(root, "commandcode", "mods", "agent-notify.ts"))
	t.Setenv("AGENT_NOTIFY_COMMANDCODE_REPLY_DIR", filepath.Join(root, "commandcode-reply"))

	binary := filepath.Join(root, "bin", "agent-notify.exe")
	writeFile(t, binary, "exe")
	return config.GetPaths(), binary
}

func writeHeartbeat(t *testing.T, path string, timestamp time.Time) {
	t.Helper()
	payload, err := json.Marshal(map[string]any{"ready": false, "timestamp": timestamp})
	if err != nil {
		t.Fatal(err)
	}
	writeFile(t, path, string(payload))
}

func writeFile(t *testing.T, path, content string) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte(content), 0600); err != nil {
		t.Fatal(err)
	}
}

func slashPath(path string) string {
	return strings.ReplaceAll(path, `\`, "/")
}

func findStatus(t *testing.T, statuses []Status, agentID string) Status {
	t.Helper()
	for _, status := range statuses {
		if status.Agent == agentID {
			return status
		}
	}
	t.Fatalf("status for %s not found", agentID)
	return Status{}
}
