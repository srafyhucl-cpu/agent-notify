//go:build windows

package setup

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"reflect"
	"testing"
)

func TestEnsureSkipsCompletedVersion(t *testing.T) {
	root := t.TempDir()
	state := filepath.Join(root, "setup-state.json")
	if err := writeState(state, "1.4.2"); err != nil {
		t.Fatal(err)
	}
	called := false
	err := ensure(context.Background(), Options{
		Version:    "1.4.2",
		InstallDir: root,
		ScriptPath: filepath.Join(root, "install.ps1"),
		StateFile:  state,
		LogFile:    filepath.Join(root, "setup.log"),
	}, func(context.Context, string, ...string) ([]byte, error) {
		called = true
		return nil, nil
	})
	if err != nil || called {
		t.Fatalf("Ensure completed err=%v called=%v", err, called)
	}
}

func TestEnsureForceRunsCompletedVersion(t *testing.T) {
	root := t.TempDir()
	state := filepath.Join(root, "setup-state.json")
	script := filepath.Join(root, "install.ps1")
	if err := writeState(state, "1.4.2"); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(script, []byte("param()"), 0600); err != nil {
		t.Fatal(err)
	}
	called := false
	err := ensure(context.Background(), Options{
		Version:    "1.4.2",
		InstallDir: root,
		ScriptPath: script,
		StateFile:  state,
		LogFile:    filepath.Join(root, "setup.log"),
		Force:      true,
	}, func(context.Context, string, ...string) ([]byte, error) {
		called = true
		return nil, nil
	})
	if err != nil || !called {
		t.Fatalf("Ensure force err=%v called=%v", err, called)
	}
}

func TestEnsureBuildsConfigureOnlyCommand(t *testing.T) {
	root := t.TempDir()
	script := filepath.Join(root, "install.ps1")
	if err := os.WriteFile(script, []byte("param()"), 0600); err != nil {
		t.Fatal(err)
	}
	var gotName string
	var gotArgs []string
	err := ensure(context.Background(), Options{
		Version:    "1.4.2",
		InstallDir: root,
		ScriptPath: script,
		StateFile:  filepath.Join(root, "setup-state.json"),
		LogFile:    filepath.Join(root, "setup.log"),
	}, func(_ context.Context, name string, args ...string) ([]byte, error) {
		gotName = name
		gotArgs = append([]string(nil), args...)
		return nil, nil
	})
	if err != nil {
		t.Fatal(err)
	}
	want := []string{
		"-NoProfile", "-ExecutionPolicy", "Bypass", "-File", script,
		"-ConfigureOnly", "-InstallDir", root,
		"-SkipWidgetLaunch", "-SkipLoginLaunch", "-SkipShortcuts",
	}
	if gotName != "powershell.exe" || !reflect.DeepEqual(gotArgs, want) {
		t.Fatalf("command = %s %#v", gotName, gotArgs)
	}
}

func TestEnsureDoesNotMarkFailureComplete(t *testing.T) {
	root := t.TempDir()
	script := filepath.Join(root, "install.ps1")
	if err := os.WriteFile(script, []byte("param()"), 0600); err != nil {
		t.Fatal(err)
	}
	state := filepath.Join(root, "setup-state.json")
	err := ensure(context.Background(), Options{
		Version:    "1.4.2",
		InstallDir: root,
		ScriptPath: script,
		StateFile:  state,
		LogFile:    filepath.Join(root, "setup.log"),
	}, func(context.Context, string, ...string) ([]byte, error) {
		return []byte("hook conflict"), errors.New("exit status 1")
	})
	if err == nil || IsComplete(state, "1.4.2") {
		t.Fatalf("Ensure failure err=%v complete=%v", err, IsComplete(state, "1.4.2"))
	}
}
