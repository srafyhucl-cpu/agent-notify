//go:build windows

package reply

import (
	"context"
	"strings"
	"syscall"
	"unicode/utf16"
	"unsafe"
)

const (
	th32csSnapProcess           = 0x00000002
	processQueryLimitedInfo     = 0x1000
	processVMRead               = 0x0010
	invalidProcessHandle        = ^uintptr(0)
	processEntryExeFileCapacity = 260
	processCommandLineMaxBytes  = 64 << 10
)

// 64 位进程的固定偏移：PEB.ProcessParameters 与
// RTL_USER_PROCESS_PARAMETERS.CommandLine。
const (
	pebProcessParametersOffset    = 0x20
	parametersCommandLineOffset   = 0x70
	processBasicInformation       = 0
	ntQueryInformationProcessArgs = 5
)

type processEntry32 struct {
	Size              uint32
	Usage             uint32
	ProcessID         uint32
	DefaultHeapID     uintptr
	ModuleID          uint32
	Threads           uint32
	ParentProcessID   uint32
	PriorityClassBase int32
	Flags             uint32
	ExeFile           [processEntryExeFileCapacity]uint16
}

// agentProcess 是一次进程枚举结果：命令行只在需要定位语言服务令牌时使用。
type agentProcess struct {
	PID         uint32
	Executable  string
	CommandLine string
}

type ntUnicodeString struct {
	Length        uint16
	MaximumLength uint16
	Buffer        *uint16
}

type processBasicInfo struct {
	Reserved1       uintptr
	PebBaseAddress  uintptr
	Reserved2       [2]uintptr
	UniqueProcessID uintptr
	Reserved3       uintptr
}

func runningAgentExecutablePaths(names ...string) []string {
	processes := runningAgentProcesses(context.Background(), names...)
	paths := make([]string, 0, len(processes))
	for _, process := range processes {
		paths = append(paths, process.Executable)
	}
	return paths
}

// runningAgentProcesses 返回指定映像名的进程列表，包含 PID 与命令行。
func runningAgentProcesses(ctx context.Context, names ...string) []agentProcess {
	if len(names) == 0 || ctx == nil {
		return nil
	}
	wanted := make(map[string]struct{}, len(names))
	for _, name := range names {
		wanted[strings.ToLower(strings.TrimSpace(name))] = struct{}{}
	}

	kernel32 := syscall.NewLazyDLL("kernel32.dll")
	createSnapshot := kernel32.NewProc("CreateToolhelp32Snapshot")
	processFirst := kernel32.NewProc("Process32FirstW")
	processNext := kernel32.NewProc("Process32NextW")
	closeHandle := kernel32.NewProc("CloseHandle")

	snapshot, _, _ := createSnapshot.Call(th32csSnapProcess, 0)
	if snapshot == 0 || snapshot == invalidProcessHandle {
		return nil
	}
	defer closeHandle.Call(snapshot)

	var entry processEntry32
	entry.Size = uint32(unsafe.Sizeof(entry))
	ok, _, _ := processFirst.Call(snapshot, uintptr(unsafe.Pointer(&entry)))
	if ok == 0 {
		return nil
	}

	var processes []agentProcess
	for {
		if err := ctx.Err(); err != nil {
			return processes
		}
		name := strings.ToLower(syscall.UTF16ToString(entry.ExeFile[:]))
		if _, match := wanted[name]; match {
			pid := entry.ProcessID
			if executable, exists := processImageName(int(pid)); exists {
				processes = append(processes, agentProcess{
					PID:         pid,
					Executable:  executable,
					CommandLine: processCommandLine(pid),
				})
			}
		}
		ok, _, _ = processNext.Call(snapshot, uintptr(unsafe.Pointer(&entry)))
		if ok == 0 {
			break
		}
	}
	return processes
}

func processCommandLine(pid uint32) string {
	if pid == 0 {
		return ""
	}
	kernel32 := syscall.NewLazyDLL("kernel32.dll")
	ntdll := syscall.NewLazyDLL("ntdll.dll")
	openProcess := kernel32.NewProc("OpenProcess")
	readProcessMemory := kernel32.NewProc("ReadProcessMemory")
	closeHandle := kernel32.NewProc("CloseHandle")
	queryInformation := ntdll.NewProc("NtQueryInformationProcess")

	handle, _, _ := openProcess.Call(
		processQueryLimitedInfo|processVMRead,
		0,
		uintptr(pid),
	)
	if handle == 0 {
		return ""
	}
	defer closeHandle.Call(handle)

	var basic processBasicInfo
	status, _, _ := queryInformation.Call(
		handle,
		processBasicInformation,
		uintptr(unsafe.Pointer(&basic)),
		unsafe.Sizeof(basic),
		0,
	)
	if int32(status) < 0 || basic.PebBaseAddress == 0 {
		return ""
	}

	var processParameters uintptr
	if !readRemote(handle, basic.PebBaseAddress+pebProcessParametersOffset, unsafe.Pointer(&processParameters), unsafe.Sizeof(processParameters), readProcessMemory) ||
		processParameters == 0 {
		return ""
	}
	var commandLine ntUnicodeString
	if !readRemote(handle, processParameters+parametersCommandLineOffset, unsafe.Pointer(&commandLine), unsafe.Sizeof(commandLine), readProcessMemory) ||
		commandLine.Buffer == nil || commandLine.Length == 0 {
		return ""
	}
	size := commandLineBufferBytes(int(commandLine.Length))
	if size < 2 {
		return ""
	}
	buffer := make([]uint16, size/2)
	if !readRemote(handle, uintptr(unsafe.Pointer(commandLine.Buffer)), unsafe.Pointer(&buffer[0]), uintptr(size), readProcessMemory) {
		return ""
	}
	return string(utf16.Decode(buffer))
}

// commandLineBufferBytes 归一化目标进程命令行长度：先截断到上限，再向下取偶，
// 保证按 uint16 读取时缓冲区与读取字节数一致；结果可能为 0。
func commandLineBufferBytes(length int) int {
	if length > processCommandLineMaxBytes {
		length = processCommandLineMaxBytes
	}
	return length &^ 1
}

func readRemote(
	handle uintptr,
	address uintptr,
	destination unsafe.Pointer,
	size uintptr,
	readProcessMemory *syscall.LazyProc,
) bool {
	if size == 0 {
		return false
	}
	var read uintptr
	result, _, _ := readProcessMemory.Call(
		handle,
		address,
		uintptr(destination),
		size,
		uintptr(unsafe.Pointer(&read)),
	)
	return result != 0 && read == size
}

// parseCommandLineFlag 读取 "--name value" 形式的命令行参数。
func parseCommandLineFlag(commandLine, name string) string {
	fields := strings.Fields(commandLine)
	for index, field := range fields {
		switch {
		case field == name && index+1 < len(fields):
			return fields[index+1]
		case strings.HasPrefix(field, name+"="):
			return strings.TrimPrefix(field, name+"=")
		}
	}
	return ""
}

const (
	mibTCPStateListen        = 2
	tcpTableOwnerPIDSize     = 24
	tcpTableOwnerPIDListener = 3
)

type mibTCPRowOwnerPID struct {
	State      uint32
	LocalAddr  uint32
	LocalPort  uint32
	RemoteAddr uint32
	RemotePort uint32
	OwningPID  uint32
}

// listeningTCPPorts 返回指定进程正在监听的本机端口，用于定位语言服务端口。
func listeningTCPPorts(pid uint32) []int {
	if pid == 0 {
		return nil
	}
	iphlpapi := syscall.NewLazyDLL("iphlpapi.dll")
	getExtendedTcpTable := iphlpapi.NewProc("GetExtendedTcpTable")

	var size uint32
	_, _, _ = getExtendedTcpTable.Call(
		0,
		uintptr(unsafe.Pointer(&size)),
		0,
		syscall.AF_INET,
		tcpTableOwnerPIDListener,
		0,
	)
	if size == 0 {
		size = tcpTableOwnerPIDSize * 64
	}
	buffer := make([]byte, size)
	status, _, _ := getExtendedTcpTable.Call(
		uintptr(unsafe.Pointer(&buffer[0])),
		uintptr(unsafe.Pointer(&size)),
		0,
		syscall.AF_INET,
		tcpTableOwnerPIDListener,
		0,
	)
	if status != 0 || len(buffer) < 4 {
		return nil
	}
	count := *(*uint32)(unsafe.Pointer(&buffer[0]))
	if count == 0 {
		return nil
	}
	rows := unsafe.Slice(
		(*mibTCPRowOwnerPID)(unsafe.Pointer(&buffer[4])),
		int(count),
	)
	seen := make(map[int]struct{}, count)
	var ports []int
	for _, row := range rows {
		if row.State != mibTCPStateListen || row.OwningPID != pid {
			continue
		}
		port := int(row.LocalPort>>8 | (row.LocalPort&0xff)<<8)
		if port <= 0 {
			continue
		}
		if _, exists := seen[port]; exists {
			continue
		}
		seen[port] = struct{}{}
		ports = append(ports, port)
	}
	return ports
}
