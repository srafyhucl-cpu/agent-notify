package marker

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestIsOff_EmptyPath(t *testing.T) {
	if IsOff("") {
		t.Error("IsOff with empty path should return false")
	}
}

func TestIsOff_NonexistentFile(t *testing.T) {
	if IsOff(filepath.Join(t.TempDir(), "nonexistent.off")) {
		t.Error("IsOff should return false for non-existent file")
	}
}

func TestIsOff_ExistingFile(t *testing.T) {
	dir := t.TempDir()
	f := filepath.Join(dir, "marker.off")
	_ = os.WriteFile(f, []byte("off"), 0644)

	if !IsOff(f) {
		t.Error("IsOff should return true when marker file exists")
	}
}

func TestSetMarker_On(t *testing.T) {
	dir := t.TempDir()
	f := filepath.Join(dir, "marker.off")
	_ = os.WriteFile(f, []byte("off"), 0644)

	status, err := SetMarker(f, "On")
	if err != nil {
		t.Fatalf("SetMarker On: %v", err)
	}
	if status != "ON" {
		t.Errorf("status = %q, want ON", status)
	}
	// File should be removed
	if _, err := os.Stat(f); !os.IsNotExist(err) {
		t.Error("marker file should be removed when turning ON")
	}
}

func TestSetMarker_Off(t *testing.T) {
	dir := t.TempDir()
	f := filepath.Join(dir, "marker.off")

	status, err := SetMarker(f, "Off")
	if err != nil {
		t.Fatalf("SetMarker Off: %v", err)
	}
	if status != "OFF" {
		t.Errorf("status = %q, want OFF", status)
	}
	// File should exist
	data, err := os.ReadFile(f)
	if err != nil {
		t.Fatalf("read marker: %v", err)
	}
	if !strings.HasPrefix(string(data), "off ") {
		t.Errorf("marker content = %q, want prefix 'off '", string(data))
	}
}

func TestSetMarker_Flip(t *testing.T) {
	dir := t.TempDir()
	f := filepath.Join(dir, "marker.off")

	// File doesn't exist → IsOff=false → flip should turn Off
	status, err := SetMarker(f, "Flip")
	if err != nil {
		t.Fatalf("Flip 1: %v", err)
	}
	if status != "OFF" {
		t.Errorf("First flip: status = %q, want OFF", status)
	}

	// File exists → IsOff=true → flip should turn On
	status, err = SetMarker(f, "Flip")
	if err != nil {
		t.Fatalf("Flip 2: %v", err)
	}
	if status != "ON" {
		t.Errorf("Second flip: status = %q, want ON", status)
	}
}

func TestSetMarker_EmptyPath(t *testing.T) {
	status, err := SetMarker("", "On")
	if err == nil {
		t.Error("SetMarker with empty path should return error")
	}
	if status != "ON" {
		t.Errorf("status = %q, want ON", status)
	}
}

func TestSetMarker_NestedDir(t *testing.T) {
	dir := t.TempDir()
	f := filepath.Join(dir, "deep", "nested", "marker.off")

	status, err := SetMarker(f, "Off")
	if err != nil {
		t.Fatalf("SetMarker nested Off: %v", err)
	}
	if status != "OFF" {
		t.Errorf("status = %q, want OFF", status)
	}
	if _, err := os.Stat(f); err != nil {
		t.Errorf("nested marker file should be created: %v", err)
	}
}

func TestSetMarker_OnAlreadyOn(t *testing.T) {
	dir := t.TempDir()
	f := filepath.Join(dir, "marker.off")
	// No file exists (already ON)

	status, err := SetMarker(f, "On")
	if err != nil {
		t.Fatalf("SetMarker On: %v", err)
	}
	if status != "ON" {
		t.Errorf("status = %q, want ON", status)
	}
}

func TestSetMarker_CaseInsensitive(t *testing.T) {
	dir := t.TempDir()
	f := filepath.Join(dir, "marker.off")

	status, _ := SetMarker(f, "OFF")
	if status != "OFF" {
		t.Errorf("OFF uppercase: status = %q, want OFF", status)
	}

	status, _ = SetMarker(f, "ON")
	if status != "ON" {
		t.Errorf("ON uppercase: status = %q, want ON", status)
	}

	status, _ = SetMarker(f, "  flip  ")
	if status != "OFF" {
		t.Errorf("flip with spaces: status = %q, want OFF", status)
	}
}
