//go:build windows

package update

import (
	"archive/zip"
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestIsNewerVersion(t *testing.T) {
	tests := []struct {
		latest  string
		current string
		want    bool
		wantErr bool
	}{
		{"1.3.0", "1.2.9", true, false},
		{"v1.3.0", "1.3.0", false, false},
		{"1.2.9", "1.3.0", false, false},
		{"2.0.0", "1.99.99", true, false},
		{"dev", "1.3.0", false, true},
	}
	for _, tt := range tests {
		got, err := isNewerVersion(tt.latest, tt.current)
		if (err != nil) != tt.wantErr || got != tt.want {
			t.Fatalf("isNewerVersion(%q,%q) = (%v,%v), want (%v,%v)", tt.latest, tt.current, got, err, tt.want, tt.wantErr)
		}
	}
}

func TestNewClientUsesUpdateOverrides(t *testing.T) {
	t.Setenv("AGENT_NOTIFY_UPDATE_REPOSITORY", "owner/repo")
	t.Setenv("AGENT_NOTIFY_UPDATE_API_BASE", "http://127.0.0.1:8080/")
	t.Setenv("AGENT_NOTIFY_GITHUB_TOKEN", "token")

	client := NewClient()
	if client.Repository != "owner/repo" || client.APIBaseURL != "http://127.0.0.1:8080" || client.Token != "token" {
		t.Fatalf("NewClient overrides = %+v", client)
	}
}

func TestCheckFindsNewerRelease(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/repos/owner/repo/releases/latest" {
			http.NotFound(w, r)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = fmt.Fprintf(w, `{
			"tag_name":"v1.4.0",
			"body":"release notes",
			"assets":[
				{"name":"Agent-notify-Setup-v1.4.0.exe","url":"%s/setup"},
				{"name":"Agent-notify-v1.4.0.zip","url":"%s/archive"},
				{"name":"SHA256SUMS.txt","url":"%s/checksums"}
			]
		}`, serverURL(r), serverURL(r), serverURL(r))
	}))
	defer server.Close()

	client := &Client{
		HTTPClient: server.Client(),
		Repository: "owner/repo",
		APIBaseURL: server.URL,
	}
	release, available, err := client.Check(context.Background(), "1.3.0")
	if err != nil {
		t.Fatalf("Check: %v", err)
	}
	if !available || release.Version != "1.4.0" {
		t.Fatalf("Check = %+v, available=%v", release, available)
	}
	if release.ArtifactKind != ArtifactInstaller || release.ArtifactURL != server.URL+"/setup" ||
		release.ChecksumURL != server.URL+"/checksums" {
		t.Fatalf("asset URLs = %+v", release)
	}

	_, available, err = client.Check(context.Background(), "1.4.0")
	if err != nil || available {
		t.Fatalf("same-version Check = available:%v err:%v", available, err)
	}
}

func TestCheckFallsBackToZipWhenSetupMissing(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/repos/owner/repo/releases/latest" {
			http.NotFound(w, r)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = fmt.Fprintf(w, `{
			"tag_name":"v1.4.0",
			"assets":[
				{"name":"Agent-notify-v1.4.0.zip","url":"%s/archive"},
				{"name":"SHA256SUMS.txt","url":"%s/checksums"}
			]
		}`, serverURL(r), serverURL(r))
	}))
	defer server.Close()

	client := &Client{
		HTTPClient: server.Client(),
		Repository: "owner/repo",
		APIBaseURL: server.URL,
	}
	release, available, err := client.Check(context.Background(), "1.3.0")
	if err != nil {
		t.Fatalf("Check: %v", err)
	}
	if !available || release.ArtifactKind != ArtifactArchive || release.ArtifactURL != server.URL+"/archive" {
		t.Fatalf("fallback release = %+v available=%v", release, available)
	}
}

func TestCheckReportsMissingPublicRelease(t *testing.T) {
	server := httptest.NewServer(http.NotFoundHandler())
	defer server.Close()

	client := &Client{
		HTTPClient: server.Client(),
		Repository: "owner/repo",
		APIBaseURL: server.URL,
	}
	_, _, err := client.Check(context.Background(), "1.3.0")
	if err == nil || !strings.Contains(err.Error(), "仓库未公开") {
		t.Fatalf("Check error = %v, want private/missing release error", err)
	}
}

func TestCheckFallsBackToReleaseRedirect(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/repos/owner/repo/releases/latest":
			http.Error(w, "rate limited", http.StatusForbidden)
		case "/owner/repo/releases/latest":
			http.Redirect(w, r, "/owner/repo/releases/tag/v1.4.0", http.StatusFound)
		case "/owner/repo/releases/tag/v1.4.0":
			w.WriteHeader(http.StatusOK)
		default:
			http.NotFound(w, r)
		}
	}))
	defer server.Close()

	client := &Client{
		HTTPClient: server.Client(),
		Repository: "owner/repo",
		APIBaseURL: server.URL,
	}
	release, available, err := client.Check(context.Background(), "1.3.0")
	if err != nil {
		t.Fatalf("Check fallback: %v", err)
	}
	if !available || release.Version != "1.4.0" {
		t.Fatalf("fallback release = %+v available=%v", release, available)
	}
	wantArchive := server.URL + "/owner/repo/releases/download/v1.4.0/Agent-notify-v1.4.0.zip"
	if release.ArtifactKind != ArtifactArchive || release.ArtifactURL != wantArchive {
		t.Fatalf("fallback archive = %+v, want URL %q", release, wantArchive)
	}
}

func TestPrepareVerifiesAndExtractsRelease(t *testing.T) {
	archive := releaseArchive(t, []zipTestFile{
		{Name: "Agent-notify/VERSION", Body: "1.4.0"},
		{Name: "Agent-notify/install.ps1", Body: "param()"},
		{Name: "Agent-notify/bin/agent-notify.exe", Body: "MZ"},
	})
	checksum := sha256.Sum256(archive)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/checksums":
			_, _ = fmt.Fprintf(w, "%s  Agent-notify-v1.4.0.zip\n", hex.EncodeToString(checksum[:]))
		case "/archive":
			_, _ = w.Write(archive)
		default:
			http.NotFound(w, r)
		}
	}))
	defer server.Close()

	client := &Client{
		HTTPClient: server.Client(),
		Repository: "owner/repo",
		APIBaseURL: server.URL,
	}
	prepared, err := client.Prepare(context.Background(), Release{
		Version:      "1.4.0",
		ArtifactKind: ArtifactArchive,
		ArtifactURL:  server.URL + "/archive",
		ChecksumURL:  server.URL + "/checksums",
	}, filepath.Join(t.TempDir(), "updates"))
	if err != nil {
		t.Fatalf("Prepare: %v", err)
	}
	if prepared.Version != "1.4.0" {
		t.Fatalf("version = %q", prepared.Version)
	}
	if prepared.Kind != ArtifactArchive || prepared.ArtifactPath == "" {
		t.Fatalf("prepared archive = %+v", prepared)
	}
	for _, path := range []string{prepared.InstallerPath, filepath.Join(prepared.StageDir, "bin", "agent-notify.exe")} {
		if _, err := os.Stat(path); err != nil {
			t.Fatalf("prepared file %s: %v", path, err)
		}
	}
}

func TestPrepareRejectsChecksumMismatch(t *testing.T) {
	archive := releaseArchive(t, []zipTestFile{
		{Name: "Agent-notify/VERSION", Body: "1.4.0"},
		{Name: "Agent-notify/install.ps1", Body: "param()"},
		{Name: "Agent-notify/bin/agent-notify.exe", Body: "MZ"},
	})
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/checksums":
			_, _ = fmt.Fprintf(w, "%064d  Agent-notify-v1.4.0.zip\n", 0)
		case "/archive":
			_, _ = w.Write(archive)
		default:
			http.NotFound(w, r)
		}
	}))
	defer server.Close()

	client := &Client{HTTPClient: server.Client(), Repository: "owner/repo", APIBaseURL: server.URL}
	_, err := client.Prepare(context.Background(), Release{
		Version:      "1.4.0",
		ArtifactKind: ArtifactArchive,
		ArtifactURL:  server.URL + "/archive",
		ChecksumURL:  server.URL + "/checksums",
	}, filepath.Join(t.TempDir(), "updates"))
	if err == nil || !strings.Contains(err.Error(), "SHA256 校验失败") {
		t.Fatalf("Prepare error = %v, want checksum mismatch", err)
	}
}

func TestPrepareVerifiesInstallerWithoutExtracting(t *testing.T) {
	installer := []byte("MZinstaller")
	checksum := sha256.Sum256(installer)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/checksums":
			_, _ = fmt.Fprintf(w, "%s  Agent-notify-Setup-v1.4.0.exe\n", hex.EncodeToString(checksum[:]))
		case "/setup":
			_, _ = w.Write(installer)
		default:
			http.NotFound(w, r)
		}
	}))
	defer server.Close()

	client := &Client{HTTPClient: server.Client(), Repository: "owner/repo", APIBaseURL: server.URL}
	prepared, err := client.Prepare(context.Background(), Release{
		Version:      "1.4.0",
		ArtifactKind: ArtifactInstaller,
		ArtifactURL:  server.URL + "/setup",
		ChecksumURL:  server.URL + "/checksums",
	}, filepath.Join(t.TempDir(), "updates"))
	if err != nil {
		t.Fatalf("Prepare: %v", err)
	}
	if prepared.Kind != ArtifactInstaller || prepared.ArtifactPath != prepared.InstallerPath {
		t.Fatalf("prepared installer = %+v", prepared)
	}
	contents, err := os.ReadFile(prepared.InstallerPath)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(contents, installer) {
		t.Fatalf("installer contents = %q", contents)
	}
	if _, err := os.Stat(filepath.Join(prepared.StageDir, "extracted")); !os.IsNotExist(err) {
		t.Fatalf("installer should not be extracted, stat err=%v", err)
	}
	if _, err := os.Stat(filepath.Join(prepared.StageDir, checksumName)); !os.IsNotExist(err) {
		t.Fatalf("installer checksum file should be removed, stat err=%v", err)
	}
}

func TestPrepareRejectsInstallerWithoutPESignature(t *testing.T) {
	installer := []byte("not a windows executable")
	checksum := sha256.Sum256(installer)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/checksums":
			_, _ = fmt.Fprintf(w, "%s  Agent-notify-Setup-v1.4.0.exe\n", hex.EncodeToString(checksum[:]))
		case "/setup":
			_, _ = w.Write(installer)
		default:
			http.NotFound(w, r)
		}
	}))
	defer server.Close()

	client := &Client{HTTPClient: server.Client(), Repository: "owner/repo", APIBaseURL: server.URL}
	_, err := client.Prepare(context.Background(), Release{
		Version:      "1.4.0",
		ArtifactKind: ArtifactInstaller,
		ArtifactURL:  server.URL + "/setup",
		ChecksumURL:  server.URL + "/checksums",
	}, filepath.Join(t.TempDir(), "updates"))
	if err == nil || !strings.Contains(err.Error(), "不是有效的 Windows 程序") {
		t.Fatalf("Prepare error = %v, want invalid PE error", err)
	}
}

func TestPrepareSupportsLegacyArchiveRelease(t *testing.T) {
	archive := releaseArchive(t, []zipTestFile{
		{Name: "Agent-notify/VERSION", Body: "1.4.0"},
		{Name: "Agent-notify/install.ps1", Body: "param()"},
		{Name: "Agent-notify/bin/agent-notify.exe", Body: "MZ"},
	})
	checksum := sha256.Sum256(archive)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/checksums":
			_, _ = fmt.Fprintf(w, "%s  Agent-notify-v1.4.0.zip\n", hex.EncodeToString(checksum[:]))
		case "/archive":
			_, _ = w.Write(archive)
		default:
			http.NotFound(w, r)
		}
	}))
	defer server.Close()

	client := &Client{HTTPClient: server.Client(), Repository: "owner/repo", APIBaseURL: server.URL}
	prepared, err := client.Prepare(context.Background(), Release{
		Version:     "1.4.0",
		ArchiveURL:  server.URL + "/archive",
		ChecksumURL: server.URL + "/checksums",
	}, filepath.Join(t.TempDir(), "updates"))
	if err != nil {
		t.Fatalf("Prepare legacy archive: %v", err)
	}
	if prepared.Kind != ArtifactArchive {
		t.Fatalf("legacy prepared kind = %q", prepared.Kind)
	}
}

func TestExtractZipRejectsTraversal(t *testing.T) {
	for _, name := range []string{"../escape.txt", `C:\escape.txt`} {
		t.Run(name, func(t *testing.T) {
			archive := releaseArchive(t, []zipTestFile{{Name: name, Body: "bad"}})
			archivePath := filepath.Join(t.TempDir(), "bad.zip")
			if err := os.WriteFile(archivePath, archive, 0600); err != nil {
				t.Fatal(err)
			}
			err := extractZip(archivePath, filepath.Join(t.TempDir(), "extract"))
			if err == nil || !strings.Contains(err.Error(), "越界路径") {
				t.Fatalf("extractZip error = %v, want traversal rejection", err)
			}
		})
	}
}

func TestBuildUpdaterScriptQuotesArguments(t *testing.T) {
	script := buildUpdaterScript(
		`C:\Users\O'Brien\Agent-notify\install.ps1`,
		`C:\Temp\update's.log`,
		[]string{"-SkipLoginLaunch", "-InstallDir", `D:\Agent's Files`},
	)
	if !strings.Contains(script, `'C:\Users\O''Brien\Agent-notify\install.ps1'`) {
		t.Fatalf("installer path was not quoted: %s", script)
	}
	if !strings.Contains(script, `'D:\Agent''s Files'`) {
		t.Fatalf("installer argument was not quoted: %s", script)
	}
	if !strings.Contains(script, `'C:\Temp\update''s.log'`) {
		t.Fatalf("log path was not quoted: %s", script)
	}
}

func TestInstallerCommandArgs(t *testing.T) {
	command := installerCommand(`D:\Temp\Agent-notify-Setup-v1.4.0.exe`, []string{
		"-SkipLoginLaunch",
		"-InstallDir",
		`D:\Agent's Files`,
	})
	if command.Path != `D:\Temp\Agent-notify-Setup-v1.4.0.exe` {
		t.Fatalf("command path = %q", command.Path)
	}
	want := []string{`D:\Temp\Agent-notify-Setup-v1.4.0.exe`, "/SILENT", "/NORESTART", "-SkipLoginLaunch", "-InstallDir", `D:\Agent's Files`}
	if fmt.Sprint(command.Args) != fmt.Sprint(want) {
		t.Fatalf("command args = %#v, want %#v", command.Args, want)
	}
	if command.SysProcAttr != nil {
		t.Fatalf("installer command must remain visible: %+v", command.SysProcAttr)
	}
}

func TestLaunchRunsInstallerInBackground(t *testing.T) {
	stageDir := t.TempDir()
	installerPath := filepath.Join(stageDir, "install.ps1")
	markerPath := filepath.Join(t.TempDir(), "installed.txt")
	script := `param([string]$Marker)
Set-Content -LiteralPath $Marker -Value "installed" -Encoding UTF8
exit 0
`
	if err := os.WriteFile(installerPath, []byte(script), 0700); err != nil {
		t.Fatal(err)
	}
	prepared := PreparedUpdate{
		Version:       "1.4.0",
		StageDir:      stageDir,
		InstallerPath: installerPath,
	}
	logPath := filepath.Join(t.TempDir(), "update.log")
	if err := prepared.Launch(logPath, "-Marker", markerPath); err != nil {
		t.Fatalf("Launch: %v", err)
	}
	deadline := time.Now().Add(15 * time.Second)
	for time.Now().Before(deadline) {
		if _, err := os.Stat(markerPath); err == nil {
			return
		}
		time.Sleep(100 * time.Millisecond)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	wrapperPath := filepath.Join(stageDir, "apply-update.ps1")
	output, runErr := exec.CommandContext(ctx, "powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", wrapperPath).CombinedOutput()
	if contents, err := os.ReadFile(logPath); err == nil {
		t.Fatalf("installer did not run; log=%s wrapper=%s runErr=%v", contents, output, runErr)
	}
	wrapper, _ := os.ReadFile(wrapperPath)
	t.Fatalf("installer did not run; wrapper=%s output=%s runErr=%v", wrapper, output, runErr)
}

type zipTestFile struct {
	Name string
	Body string
}

func releaseArchive(t *testing.T, files []zipTestFile) []byte {
	t.Helper()
	var buffer bytes.Buffer
	writer := zip.NewWriter(&buffer)
	for _, file := range files {
		entry, err := writer.Create(file.Name)
		if err != nil {
			t.Fatal(err)
		}
		if _, err := entry.Write([]byte(file.Body)); err != nil {
			t.Fatal(err)
		}
	}
	if err := writer.Close(); err != nil {
		t.Fatal(err)
	}
	return buffer.Bytes()
}

func serverURL(request *http.Request) string {
	return "http://" + request.Host
}
