package main

import (
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"syscall"
	"time"

	"linkweixin/internal/agent"
	"linkweixin/internal/notify"
	"linkweixin/internal/ui"
)

var (
	kernel32       = syscall.NewLazyDLL("kernel32.dll")
	pAttachConsole = kernel32.NewProc("AttachConsole")
)

func attachParentConsole() {
	const ATTACH_PARENT_PROCESS = ^uintptr(0) // -1
	ret, _, _ := pAttachConsole.Call(ATTACH_PARENT_PROCESS)
	if ret != 0 {
		const STD_OUTPUT_HANDLE = ^uintptr(10) // -11
		hStdOut, _, _ := kernel32.NewProc("GetStdHandle").Call(STD_OUTPUT_HANDLE)
		if hStdOut == 0 || hStdOut == ^uintptr(0) {
			if conout, err := os.OpenFile("CONOUT$", os.O_WRONLY, 0644); err == nil {
				os.Stdout = conout
				os.Stderr = conout
			}
		}
	}
}

func printHelp() {
	fmt.Printf("linkWeixin v%s - 原生独立单文件 AI 任务推送助手\n\n", ui.AppVersion)
	fmt.Println("用法:")
	fmt.Println("  linkweixin.exe [子命令] [选项]")
	fmt.Println()
	fmt.Println("子命令:")
	fmt.Println("  widget / gui    启动桌面微光悬浮窗与系统折叠托盘（默认）")
	fmt.Println("  notify          触发多通道推送（支持 OpenCode 任务及管道输入）")
	fmt.Println("  codex           OpenAI Codex 通知中转（透传电脑操控并推送微信）")
	fmt.Println("  antigravity     Google Antigravity Stop 钩子拦截器（流式提取任务并推送）")
	fmt.Println("  toggle          推送开关切换（支持 all / opencode / codex / antigravity）")
	fmt.Println("  watch           看护 Codex notify 配置与悬浮窗存活状态")
	fmt.Println("  history         查看最近的推送历史记录")
	fmt.Println("  version         打印当前版本信息")
	fmt.Println()
	fmt.Println("notify 选项:")
	fmt.Println("  -title <string>       任务标题（默认：【AI任务】跑完了）")
	fmt.Println("  -summary <string>     摘要内容（支持 Markdown，为空时可读管道 stdin）")
	fmt.Println("  -max-chars <int>      摘要最大截断字数（默认：500）")
	fmt.Println("  -dry-run              仅输出脱敏 Payload，不进行网络发送")
	fmt.Println("  -no-stdin             禁用从 stdin 读取摘要")
	fmt.Println()
	fmt.Println("toggle 选项:")
	fmt.Println("  -agent <name>         指定 Agent（all, opencode, codex, antigravity，默认 all）")
	fmt.Println("  -on                   开启推送")
	fmt.Println("  -off                  关闭推送（默认: 翻转状态）")
}

func main() {
	runtime.LockOSThread()
	f, _ := os.OpenFile(filepath.Join(os.TempDir(), "linkweixin-boot.log"), os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0644)
	if f != nil {
		_, _ = f.WriteString(fmt.Sprintf("%s main: %s\n", time.Now().Format(time.RFC3339), strings.Join(os.Args, " ")))
		_ = f.Close()
	}

	args := os.Args[1:]

	// If no arguments provided
	if len(args) == 0 {
		// Check if stdin is piped
		fi, err := os.Stdin.Stat()
		if err == nil && (fi.Mode()&os.ModeCharDevice) == 0 {
			attachParentConsole()
			agent.HandleOpenCode("【AI任务】跑完了", "", 500, false, false)
			return
		}
		// Otherwise launch GUI widget
		ui.RunWidget()
		return
	}

	subCmd := strings.ToLower(args[0])

	// Direct codex hook compatibility: if first arg is "turn-ended", treat as codex subcmd
	if subCmd == "turn-ended" {
		agent.HandleCodex(args)
		return
	}

	switch subCmd {
	case "widget", "gui":
		ui.RunWidget()

	case "notify":
		attachParentConsole()
		title := "【AI任务】跑完了"
		summary := ""
		maxChars := 500
		dryRun := false
		noStdin := false
		for i := 1; i < len(args); i++ {
			a := strings.ToLower(args[i])
			if a == "-title" || a == "--title" {
				if i+1 < len(args) {
					title = args[i+1]
					i++
				}
			} else if a == "-summary" || a == "--summary" {
				if i+1 < len(args) {
					summary = args[i+1]
					i++
				}
			} else if a == "-max-chars" || a == "--max-chars" || a == "-maxchars" {
				if i+1 < len(args) {
					if n, err := strconv.Atoi(args[i+1]); err == nil && n > 0 {
						maxChars = n
					}
					i++
				}
			} else if a == "-dry-run" || a == "--dry-run" || a == "-dryrun" {
				dryRun = true
			} else if a == "-no-stdin" || a == "--no-stdin" || a == "-nostdin" {
				noStdin = true
			}
		}
		agent.HandleOpenCode(title, summary, maxChars, dryRun, noStdin)

	case "codex":
		agent.HandleCodex(args[1:])

	case "antigravity":
		attachParentConsole()
		dryRun := false
		payload := ""
		for i := 1; i < len(args); i++ {
			a := strings.ToLower(args[i])
			if a == "-dry-run" || a == "--dry-run" || a == "-dryrun" {
				dryRun = true
			} else if !strings.HasPrefix(a, "-") && payload == "" {
				payload = args[i]
			}
		}
		agent.HandleAntigravity(payload, dryRun)

	case "toggle":
		attachParentConsole()
		agentName := "all"
		mode := "Flip"
		for i := 1; i < len(args); i++ {
			a := strings.ToLower(args[i])
			if a == "-on" || a == "--on" {
				mode = "On"
			} else if a == "-off" || a == "--off" {
				mode = "Off"
			} else if a == "-agent" || a == "--agent" {
				if i+1 < len(args) {
					agentName = args[i+1]
					i++
				}
			} else if !strings.HasPrefix(a, "-") && agentName == "all" {
				agentName = args[i]
			}
		}
		agent.HandleToggle(agentName, mode)

	case "watch":
		configPath := ""
		if len(args) > 1 && !strings.HasPrefix(args[1], "-") {
			configPath = args[1]
		}
		agent.HandleWatch(configPath, "")

	case "history":
		attachParentConsole()
		limit := 50
		if len(args) > 1 {
			if n, err := strconv.Atoi(args[1]); err == nil && n > 0 {
				limit = n
			}
		}
		records, err := notify.GetHistory(limit, "")
		if err != nil {
			fmt.Printf("读取历史日志失败: %v\n", err)
			return
		}
		if len(records) == 0 {
			fmt.Println("暂无推送历史记录。")
			return
		}
		fmt.Printf("最近 %d 条推送历史:\n", len(records))
		fmt.Printf("%-20s | %-16s | %-10s | %-8s | %s\n", "时间", "标题", "通道", "状态", "摘要")
		fmt.Println(strings.Repeat("-", 80))
		for _, r := range records {
			fmt.Printf("%-20s | %-16s | %-10s | %-8s | %s\n", r.Time, r.Title, r.Channels, r.Status, r.Summary)
		}

	case "version", "-v", "--version":
		attachParentConsole()
		fmt.Printf("linkWeixin v%s (Go Native Single Executable)\n", ui.AppVersion)

	case "help", "-h", "--help":
		attachParentConsole()
		printHelp()

	default:
		attachParentConsole()
		fmt.Printf("未知子命令: %s\n\n", args[0])
		printHelp()
	}
}
