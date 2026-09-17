//go:build windows

package reply

import "testing"

func TestCommandLineBufferBytes(t *testing.T) {
	tests := []struct {
		name   string
		length int
		want   int
	}{
		{name: "zero", length: 0, want: 0},
		{name: "one", length: 1, want: 0},
		{name: "two", length: 2, want: 2},
		{name: "three", length: 3, want: 2},
		{name: "odd", length: 9, want: 8},
		{name: "at limit", length: processCommandLineMaxBytes, want: processCommandLineMaxBytes},
		{name: "above limit", length: processCommandLineMaxBytes + 1, want: processCommandLineMaxBytes},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			if got := commandLineBufferBytes(test.length); got != test.want {
				t.Fatalf("commandLineBufferBytes(%d) = %d, want %d", test.length, got, test.want)
			}
		})
	}
}
