package agent

import (
	"os"
	"syscall"
	"unicode/utf8"
	"unsafe"
)

var (
	kernel32             = syscall.NewLazyDLL("kernel32.dll")
	pMultiByteToWideChar = kernel32.NewProc("MultiByteToWideChar")
)

// DecodeConsoleBytes converts byte input to UTF-8 string, auto-detecting GBK/ANSI if invalid UTF-8.
func DecodeConsoleBytes(data []byte) string {
	if len(data) == 0 {
		return ""
	}
	if utf8.Valid(data) {
		return string(data)
	}

	// CP_ACP = 0
	wlen, _, _ := pMultiByteToWideChar.Call(0, 0, uintptr(unsafe.Pointer(&data[0])), uintptr(len(data)), 0, 0)
	if wlen == 0 {
		return string(data)
	}

	buf := make([]uint16, wlen)
	pMultiByteToWideChar.Call(0, 0, uintptr(unsafe.Pointer(&data[0])), uintptr(len(data)), uintptr(unsafe.Pointer(&buf[0])), wlen)
	return syscall.UTF16ToString(buf)
}

// ReadPipedStdinNonBlocking safely reads piped stdin without hanging when no input is available.
func ReadPipedStdinNonBlocking() []byte {
	fi, err := os.Stdin.Stat()
	if err != nil || (fi.Mode()&os.ModeCharDevice) != 0 {
		return nil
	}

	pPeekNamedPipe := kernel32.NewProc("PeekNamedPipe")
	var bytesAvail uint32
	ret, _, _ := pPeekNamedPipe.Call(os.Stdin.Fd(), 0, 0, 0, uintptr(unsafe.Pointer(&bytesAvail)), 0)
	if ret == 0 || bytesAvail == 0 {
		return nil
	}

	buf := make([]byte, bytesAvail)
	n, _ := os.Stdin.Read(buf)
	return stdinReadResult(buf, n)
}

// stdinReadResult 决定一次 stdin 读取的结果：只要读到字节就保留。
// Read 允许在返回若干字节的同时带回 EOF 等错误（同一调用内的非 nil error），
// 这些字节仍是有效输入，丢弃会丢失本次摘要；只有一字节都没读到才视为没有 stdin。
func stdinReadResult(buffer []byte, read int) []byte {
	if read <= 0 {
		return nil
	}
	return buffer[:read]
}
