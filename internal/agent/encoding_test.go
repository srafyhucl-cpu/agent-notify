package agent

import (
	"os"
	"testing"
)

// TestStdinReadResult 锁定「读到字节就保留」的契约：Read 可能在同一调用里
// 既返回字节又返回 EOF 等错误，这些字节不能被丢弃。
func TestStdinReadResult(t *testing.T) {
	buffer := []byte("hello")
	tests := []struct {
		name string
		read int
		want string
	}{
		{name: "no bytes", read: 0, want: ""},
		{name: "negative", read: -1, want: ""},
		{name: "partial", read: 3, want: "hel"},
		{name: "all", read: 5, want: "hello"},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			got := stdinReadResult(buffer, test.read)
			if string(got) != test.want {
				t.Fatalf("stdinReadResult(%d) = %q, want %q", test.read, got, test.want)
			}
		})
	}
}

func TestReadPipedStdinNonBlockingReadsPipe(t *testing.T) {
	reader, writer, err := os.Pipe()
	if err != nil {
		t.Fatalf("os.Pipe: %v", err)
	}
	defer reader.Close()
	defer writer.Close()

	const payload = "摘要内容"
	if _, err := writer.Write([]byte(payload)); err != nil {
		t.Fatalf("write pipe: %v", err)
	}

	previous := os.Stdin
	os.Stdin = reader
	defer func() { os.Stdin = previous }()

	if got := ReadPipedStdinNonBlocking(); string(got) != payload {
		t.Fatalf("ReadPipedStdinNonBlocking() = %q, want %q", got, payload)
	}
}

func TestReadPipedStdinNonBlockingEmptyPipe(t *testing.T) {
	reader, writer, err := os.Pipe()
	if err != nil {
		t.Fatalf("os.Pipe: %v", err)
	}
	defer writer.Close()

	previous := os.Stdin
	os.Stdin = reader
	defer func() {
		os.Stdin = previous
		_ = reader.Close()
	}()

	if got := ReadPipedStdinNonBlocking(); got != nil {
		t.Fatalf("ReadPipedStdinNonBlocking() = %q, want nil for empty pipe", got)
	}
}
