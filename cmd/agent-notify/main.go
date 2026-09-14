package main

//go:generate go run github.com/akavel/rsrc@v0.10.2 -manifest agent-notify.manifest -arch 386 -o rsrc_windows_386.syso
//go:generate go run github.com/akavel/rsrc@v0.10.2 -manifest agent-notify.manifest -arch amd64 -o rsrc_windows_amd64.syso
//go:generate go run github.com/akavel/rsrc@v0.10.2 -manifest agent-notify.manifest -arch arm64 -o rsrc_windows_arm64.syso

import (
	"bufio"
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"syscall"
	"time"

	"github.com/srafyhucl-cpu/agent-notify/internal/agent"
	"github.com/srafyhucl-cpu/agent-notify/internal/agentmeta"
	"github.com/srafyhucl-cpu/agent-notify/internal/app"
	"github.com/srafyhucl-cpu/agent-notify/internal/clawbot"
	"github.com/srafyhucl-cpu/agent-notify/internal/config"
	"github.com/srafyhucl-cpu/agent-notify/internal/marker"
	"github.com/srafyhucl-cpu/agent-notify/internal/notify"
	"github.com/srafyhucl-cpu/agent-notify/internal/reply"
	"github.com/srafyhucl-cpu/agent-notify/internal/ui"
)

var (
	kernel32            = syscall.NewLazyDLL("kernel32.dll")
	pAttachConsole      = kernel32.NewProc("AttachConsole")
	pAllocConsole       = kernel32.NewProc("AllocConsole")
	pGetConsoleWindow   = kernel32.NewProc("GetConsoleWindow")
	pSetConsoleOutputCP = kernel32.NewProc("SetConsoleOutputCP")
	pGetStdHandle       = kernel32.NewProc("GetStdHandle")
	pGetFileType        = kernel32.NewProc("GetFileType")
)

const (
	stdInputHandle  = ^uintptr(9)  // -10
	stdOutputHandle = ^uintptr(10) // -11
	stdErrorHandle  = ^uintptr(11) // -12

	doctorCodexQueueTimeout = 6 * time.Second
)

func bindConsole() {
	_, _, _ = pSetConsoleOutputCP.Call(65001)
	if !standardHandleUsable(stdOutputHandle) {
		if output, err := os.OpenFile("CONOUT$", os.O_WRONLY, 0600); err == nil {
			os.Stdout = output
		}
	}
	if !standardHandleUsable(stdErrorHandle) {
		if output, err := os.OpenFile("CONOUT$", os.O_WRONLY, 0600); err == nil {
			os.Stderr = output
		}
	}
	if !standardHandleUsable(stdInputHandle) {
		if input, err := os.OpenFile("CONIN$", os.O_RDONLY, 0600); err == nil {
			os.Stdin = input
		}
	}
}

func standardHandleUsable(kind uintptr) bool {
	if pGetStdHandle.Find() != nil || pGetFileType.Find() != nil {
		return false
	}
	handle, _, _ := pGetStdHandle.Call(kind)
	if handle == 0 || handle == ^uintptr(0) {
		return false
	}
	fileType, _, _ := pGetFileType.Call(handle)
	return fileType != 0
}

func attachConsole() bool {
	if standardHandleUsable(stdOutputHandle) {
		return true
	}
	if window, _, _ := pGetConsoleWindow.Call(); window != 0 {
		bindConsole()
		return true
	}
	const attachParentProcess = ^uintptr(0)
	if result, _, _ := pAttachConsole.Call(attachParentProcess); result != 0 {
		bindConsole()
		return true
	}
	return false
}

// ensureConsole attaches to a parent terminal or opens one when the binary was
// started from Explorer. Hook commands use attachConsole so an agent-spawned
// notify process never flashes a fresh console window.
func ensureConsole() {
	if attachConsole() {
		return
	}
	if result, _, _ := pAllocConsole.Call(); result != 0 {
		bindConsole()
	}
}

func printHelp() {
	fmt.Printf("Agent-notify v%s - OpenCode / Codex / Antigravity / Devin 微信任务通知\n\n", app.Version)
	fmt.Println("用法:")
	fmt.Println("  agent-notify [命令] [选项]")
	fmt.Println()
	fmt.Println("命令:")
	fmt.Println("  login       微信扫码登录 ClawBot，默认随后等待首条消息")
	fmt.Println("  sync        等待首条微信消息，建立主动推送会话")
	fmt.Println("  logout      删除本机 ClawBot 凭据")
	fmt.Println("  status      查看登录、会话、开关、配置与运行状态")
	fmt.Println("  notify      发送一条通知（供脚本或插件调用）")
	fmt.Println("  test        发送测试通知并验证完整链路")
	fmt.Println("  doctor      检查配置、凭据、会话、网络和 Codex 接入")
	fmt.Println("  reply-check 只读校验微信引用 ID 与通知发送记录是否精确对应")
	fmt.Println("  toggle      开启或暂停 OpenCode / Codex / Antigravity / Devin 推送")
	fmt.Println("  watch       检查并恢复 Codex notify 配置")
	fmt.Println("  history     查看最近推送记录（--json 供脚本消费）")
	fmt.Println("  widget      启动桌面悬浮窗（无参数时默认）")
	fmt.Println("  version     打印版本、提交和构建时间")
	fmt.Println()
	fmt.Println("常用示例:")
	fmt.Println("  agent-notify login")
	fmt.Println("  agent-notify sync")
	fmt.Println("  agent-notify test")
	fmt.Println("  agent-notify history --limit 5 --json")
	fmt.Println(`  agent-notify notify --title "构建完成" --summary "Release 已生成"`)
	fmt.Println("  agent-notify toggle --agent all --off")
}

func main() {
	if len(os.Args) < 2 {
		if stdinAvailable() {
			result := agent.HandleNotify("", "【通知】任务完成", "", "", notify.DefaultMaxChars, false, false)
			if result.Error != "" && result.Status != notify.StatusSkipped {
				fmt.Fprintln(os.Stderr, result.Error)
			}
			return
		}
		ui.RunWidget()
		return
	}

	command := strings.ToLower(os.Args[1])
	args := os.Args[2:]
	switch command {
	case "login":
		ensureConsole()
		runLogin(args)
	case "sync":
		ensureConsole()
		runSync(args)
	case "logout":
		ensureConsole()
		runLogout(args)
	case "status":
		ensureConsole()
		runStatus(args)
	case "notify":
		attachConsole()
		runNotify(args)
	case "test":
		ensureConsole()
		runTest()
	case "doctor":
		ensureConsole()
		if runDoctor() != 0 {
			os.Exit(1)
		}
	case "reply-check", "replycheck":
		ensureConsole()
		if code := runReplyCheck(args); code != 0 {
			os.Exit(code)
		}
	case "toggle":
		ensureConsole()
		if runToggle(args) != 0 {
			os.Exit(1)
		}
	case "codex":
		attachConsole()
		result := agent.HandleCodex(args)
		if result.DryRunPayload != "" {
			fmt.Println(result.DryRunPayload)
		}
	case "antigravity":
		attachConsole()
		runStopHook(args, agent.HandleAntigravityStop)
	case "devin":
		attachConsole()
		runStopHook(args, agent.HandleDevinStop)
	case "watch":
		ensureConsole()
		if runWatch(args) != 0 {
			os.Exit(1)
		}
	case "history":
		ensureConsole()
		if runHistory(args) != 0 {
			os.Exit(1)
		}
	case "widget":
		ui.RunWidget()
	case "version", "-v", "--version":
		ensureConsole()
		fmt.Printf("Agent-notify %s\ncommit: %s\nbuilt: %s\n", app.Version, app.Commit, app.BuildTime)
	case "help", "-h", "--help":
		ensureConsole()
		printHelp()
	default:
		ensureConsole()
		fmt.Fprintf(os.Stderr, "未知命令: %s\n\n", command)
		printHelp()
		os.Exit(1)
	}
}

func stdinAvailable() bool {
	info, err := os.Stdin.Stat()
	return err == nil && (info.Mode()&os.ModeCharDevice) == 0
}

func runStopHook(args []string, handle func(io.Reader) notify.NotifyResult) {
	// Stop hooks are synchronous in both agents. Always emit the accepted
	// empty JSON object even when parsing, policy, or delivery fails.
	defer fmt.Println("{}")
	if len(args) == 0 || !strings.EqualFold(strings.TrimSpace(args[0]), "stop") {
		return
	}
	_ = handle(os.Stdin)
}

func runLogin(args []string) {
	flags := flag.NewFlagSet("login", flag.ContinueOnError)
	timeout := flags.Duration("timeout", 5*time.Minute, "等待扫码确认的最长时间")
	wait := flags.Bool("wait", true, "登录后等待首条微信消息以建立主动推送会话")
	waitTimeout := flags.Duration("wait-timeout", 3*time.Minute, "等待首条微信消息的最长时间")
	if err := flags.Parse(args); err != nil {
		return
	}

	ctx, cancel := context.WithTimeout(context.Background(), *timeout)
	defer cancel()
	input := bufio.NewReader(os.Stdin)

	credentials, err := clawbot.Login(ctx, clawbot.LoginOptions{
		OnQRCode: func(response clawbot.QRCodeResponse) error {
			rendered, err := clawbot.RenderQR(response.DisplayContent())
			if err != nil {
				return err
			}
			fmt.Println("请使用微信 ClawBot 扫描以下二维码：")
			fmt.Println(rendered)
			return nil
		},
		OnStatus: func(status string) {
			if text := loginStatusText(status); text != "" {
				fmt.Println("状态：" + text)
			}
		},
		VerifyCode: func(retry bool) (string, error) {
			prompt := "请输入手机微信显示的数字配对码: "
			if retry {
				prompt = "配对码不匹配，请重新输入: "
			}
			fmt.Print(prompt)
			line, err := input.ReadString('\n')
			if err != nil {
				return "", err
			}
			return strings.TrimSpace(line), nil
		},
	})
	if err != nil {
		fmt.Fprintf(os.Stderr, "登录失败: %v\n", err)
		os.Exit(1)
	}
	if err := clawbot.SaveCredentials(credentials); err != nil {
		fmt.Fprintf(os.Stderr, "保存凭据失败: %v\n", err)
		os.Exit(1)
	}
	fmt.Printf("ClawBot 登录成功。凭据已保存到 %s\n", clawbot.CredentialsPath())

	if !*wait {
		fmt.Println("下一步：在微信中给 ClawBot 发送任意一条消息，然后运行 agent-notify sync 建立主动推送会话。")
		return
	}

	waitCtx, waitCancel := context.WithTimeout(context.Background(), *waitTimeout)
	defer waitCancel()
	fmt.Println("下一步：请在微信中给 ClawBot 发送任意一条消息，用于建立主动推送会话…")
	if _, err := clawbot.AwaitSessionContext(waitCtx, nil); err != nil {
		fmt.Println("尚未收到微信消息，主动推送会话还未建立。")
		fmt.Println("在微信中给 ClawBot 发一条消息后，运行 agent-notify sync 即可完成。")
		return
	}
	fmt.Println("主动推送会话已建立，可以发送微信通知了。")
}

// runSync waits until the bound WeChat account sends one message, which is what
// makes proactive ClawBot sends possible.
func runSync(args []string) {
	flags := flag.NewFlagSet("sync", flag.ContinueOnError)
	timeout := flags.Duration("timeout", 5*time.Minute, "等待首条消息的最长时间")
	if err := flags.Parse(args); err != nil {
		return
	}

	status := clawbot.GetStatus()
	switch {
	case !status.LoggedIn:
		fmt.Fprintln(os.Stderr, "尚未登录 ClawBot，请先运行 agent-notify login。")
		os.Exit(1)
	case status.Stale:
		fmt.Fprintln(os.Stderr, "ClawBot 登录已失效，请重新运行 agent-notify login。")
		os.Exit(1)
	case status.SessionReady:
		fmt.Println("主动推送会话已就绪。")
		return
	}

	ctx, cancel := context.WithTimeout(context.Background(), *timeout)
	defer cancel()
	fmt.Println("请在微信中给 ClawBot 发送任意一条消息…")
	if _, err := clawbot.AwaitSessionContext(ctx, nil); err != nil {
		fmt.Fprintf(os.Stderr, "建立主动推送会话失败: %v\n", err)
		os.Exit(1)
	}
	fmt.Println("主动推送会话已建立。")
}

func loginStatusText(status string) string {
	switch status {
	case clawbot.StatusWait:
		return "等待扫码"
	case clawbot.StatusScanned:
		return "已扫码，请在微信中确认"
	case clawbot.StatusScannedRedirect:
		return "正在切换扫码节点"
	case clawbot.StatusNeedVerifyCode:
		return "需要在微信中查看数字配对码"
	case clawbot.StatusConfirmed:
		return "已确认"
	case clawbot.StatusExpired:
		return "二维码已过期，正在重新获取"
	case clawbot.StatusVerifyBlocked:
		return "配对码错误次数过多，正在重新获取二维码"
	default:
		return ""
	}
}

func runLogout(args []string) {
	flags := flag.NewFlagSet("logout", flag.ContinueOnError)
	yes := flags.Bool("yes", false, "不询问，直接删除本机凭据")
	if err := flags.Parse(args); err != nil {
		return
	}
	if !*yes {
		fmt.Print("确定删除本机 ClawBot 凭据吗？输入 yes 继续: ")
		answer, _ := bufio.NewReader(os.Stdin).ReadString('\n')
		if strings.ToLower(strings.TrimSpace(answer)) != "yes" {
			fmt.Println("已取消。")
			return
		}
	}
	if err := clawbot.DeleteCredentials(); err != nil {
		fmt.Fprintf(os.Stderr, "退出登录失败: %v\n", err)
		os.Exit(1)
	}
	fmt.Println("ClawBot 凭据已删除。")
}

func runStatus(args []string) {
	flags := flag.NewFlagSet("status", flag.ContinueOnError)
	asJSON := flags.Bool("json", false, "以 JSON 输出")
	if err := flags.Parse(args); err != nil {
		return
	}

	paths := config.GetPaths()
	cfg, cfgErr := config.LoadConfig("")
	status := clawbot.GetStatus()
	openCodeOn := !marker.IsOff(paths.OpenCodeMarker)
	codexOn := !marker.IsOff(paths.CodexMarker)
	antigravityOn := !marker.IsOff(paths.AntigravityMarker)
	devinOn := !marker.IsOff(paths.DevinMarker)
	history, _ := notify.GetHistory(1, paths.PushLog)

	output := map[string]interface{}{
		"version":            app.Version,
		"commit":             app.Commit,
		"configFile":         paths.ConfigFile,
		"credentialFile":     paths.CredentialFile,
		"clawbot":            status,
		"quietHours":         cfg.QuietHours,
		"cooldownMinutes":    cfg.CooldownMin,
		"replyEnabled":       cfg.ReplyEnabled,
		"openCodeEnabled":    openCodeOn,
		"codexEnabled":       codexOn,
		"antigravityEnabled": antigravityOn,
		"devinEnabled":       devinOn,
		"pushLog":            paths.PushLog,
		"lastPush":           firstHistory(history),
		"pluginFile":         paths.PluginFile,
		"pluginInstalled":    fileExists(paths.PluginFile),
		"replyRouteFile":     paths.ReplyRouteFile,
		"codexTitleLog":      paths.CodexTitleLog,
		"antigravityHooks":   paths.AntigravityHooks,
		"devinConfig":        paths.DevinConfig,
	}
	if cfgErr != nil {
		output["configError"] = cfgErr.Error()
	}

	if *asJSON {
		data, _ := json.MarshalIndent(output, "", "  ")
		fmt.Println(string(data))
		return
	}

	fmt.Printf("Agent-notify v%s\n", app.Version)
	fmt.Printf("ClawBot: %s\n", loginStatus(status))
	fmt.Printf("主动推送会话: %s\n", sessionStatus(status))
	fmt.Printf("OpenCode 推送: %s\n", onOff(openCodeOn))
	fmt.Printf("Codex 推送: %s\n", onOff(codexOn))
	fmt.Printf("Antigravity 推送: %s\n", onOff(antigravityOn))
	fmt.Printf("Devin 推送: %s\n", onOff(devinOn))
	fmt.Printf("引用回复: %s\n", onOff(cfg.ReplyEnabled))
	fmt.Printf("勿扰时段: %s\n", emptyAs(cfg.QuietHours, "关闭"))
	fmt.Printf("会话冷却: %d 分钟\n", cfg.CooldownMin)
	fmt.Printf("配置文件: %s\n", paths.ConfigFile)
	fmt.Printf("凭据文件: %s\n", paths.CredentialFile)
	fmt.Printf("OpenCode 插件: %s\n", installedStatus(fileExists(paths.PluginFile)))
	fmt.Printf("Antigravity Hook: %s\n", paths.AntigravityHooks)
	fmt.Printf("Devin Hook: %s\n", paths.DevinConfig)
	fmt.Printf("推送日志: %s\n", paths.PushLog)
	fmt.Printf("引用路由文件: %s\n", paths.ReplyRouteFile)
	fmt.Printf("Codex 标题诊断: %s\n", paths.CodexTitleLog)
	if cfgErr != nil {
		fmt.Printf("配置错误: %v\n", cfgErr)
	}
}

func runNotify(args []string) {
	flags := flag.NewFlagSet("notify", flag.ContinueOnError)
	agentName := flags.String("agent", "", "来源标识，例如 opencode")
	title := flags.String("title", "", "通知标题；为空时按 agent 使用默认标题")
	summary := flags.String("summary", "", "通知摘要；为空时读取 stdin")
	sessionID := flags.String("session", "", "会话 ID，用于记录来源")
	maxChars := flags.Int("max-chars", notify.DefaultMaxChars, "最终消息最大字符数，0 表示不限")
	dryRun := flags.Bool("dry-run", false, "只输出消息，不发送")
	noStdin := flags.Bool("no-stdin", false, "禁止读取 stdin")
	if err := flags.Parse(args); err != nil {
		return
	}

	result := agent.HandleNotify(*agentName, *title, *summary, *sessionID, *maxChars, *dryRun, *noStdin)
	if result.Error != "" && result.Status != notify.StatusSkipped && !*dryRun {
		fmt.Fprintln(os.Stderr, result.Error)
	}
}

func runTest() {
	result := notify.SendNotification(notify.NotifyOptions{
		Agent:   "test",
		Title:   "【测试】Agent-notify",
		Summary: "如果你在微信中看到这条消息，说明 ClawBot 登录与发送链路正常。",
	})
	if result.Status != notify.StatusSuccess {
		fmt.Fprintf(os.Stderr, "测试失败: %s", result.Status)
		if result.Error != "" {
			fmt.Fprintf(os.Stderr, " - %s", result.Error)
		}
		fmt.Fprintln(os.Stderr)
		os.Exit(1)
	}
	fmt.Println("测试通知已发送。")
}

func runDoctor() int {
	paths := config.GetPaths()
	failures := 0
	fmt.Printf("Agent-notify doctor v%s\n\n", app.Version)

	if err := os.MkdirAll(paths.ConfigDir, 0700); err != nil {
		reportCheck(false, "配置目录可写", err.Error())
		failures++
	} else {
		probe := filepath.Join(paths.ConfigDir, ".write-test")
		if err := os.WriteFile(probe, []byte("ok"), 0600); err != nil {
			reportCheck(false, "配置目录可写", err.Error())
			failures++
		} else {
			_ = os.Remove(probe)
			reportCheck(true, "配置目录可写", paths.ConfigDir)
		}
	}

	cfg, err := config.LoadConfig("")
	if err != nil {
		reportCheck(false, "配置文件格式", err.Error())
		failures++
	} else {
		reportCheck(true, "配置文件格式", fmt.Sprintf("quiet=%s cooldown=%d", emptyAs(cfg.QuietHours, "关闭"), cfg.CooldownMin))
	}

	status := clawbot.GetStatus()
	switch {
	case !status.LoggedIn:
		reportCheck(false, "ClawBot 登录", "未登录，请运行 agent-notify login")
		failures++
		reportCheck(false, "主动推送会话", "未登录")
		failures++
	case status.Stale:
		reportCheck(false, "ClawBot 登录", "登录已失效，请重新运行 agent-notify login")
		failures++
		reportCheck(false, "主动推送会话", "登录已失效")
		failures++
	default:
		reportCheck(true, "ClawBot 登录", status.UserHint)
		if status.SessionReady {
			reportCheck(true, "主动推送会话", "已就绪")
		} else {
			reportCheck(false, "主动推送会话", "请在微信中给 ClawBot 发送一条消息，再运行 agent-notify sync")
			failures++
		}
	}

	baseURL := clawbot.DefaultBaseURL
	if credentials, err := clawbot.LoadCredentials(); err == nil && credentials.BaseURL != "" {
		baseURL = credentials.BaseURL
	}
	probeCtx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	err = clawbot.Probe(probeCtx, baseURL)
	cancel()
	if err != nil {
		reportCheck(false, "ClawBot 网络", err.Error())
		failures++
	} else {
		reportCheck(true, "ClawBot 网络", baseURL)
	}

	if fileExists(paths.PluginFile) {
		reportCheck(true, "OpenCode 插件", paths.PluginFile)
	} else {
		reportCheck(false, "OpenCode 插件", "未安装，请重新运行 install.ps1")
		failures++
	}

	reportHookIntegration("Antigravity 接入", paths.AntigravityHooks, "antigravity stop")
	reportHookIntegration("Devin 接入", paths.DevinConfig, "devin stop")

	codexConfig := codexConfigPath()
	codexInUse := false
	if data, err := os.ReadFile(codexConfig); err != nil {
		if os.IsNotExist(err) {
			reportCheck(true, "Codex 接入", "未发现 config.toml，按未使用处理")
		} else {
			reportCheck(false, "Codex 接入", err.Error())
			failures++
		}
	} else {
		content := strings.ToLower(string(data))
		switch {
		case strings.Contains(content, "agent-notify"):
			codexInUse = true
			reportCheck(true, "Codex 接入", codexConfig)
		case strings.Contains(content, "codex-computer-use.exe"):
			codexInUse = true
			reportCheck(false, "Codex 接入", "notify 仍直指上游程序，请运行 agent-notify watch")
			failures++
		case strings.Contains(content, "notify"):
			codexInUse = true
			reportCheck(true, "Codex 接入", "自定义 notify 保持不变")
		default:
			reportCheck(false, "Codex 接入", "config.toml 未配置 notify")
			failures++
		}
	}
	failures += checkCodexReplySupport(cfg, codexInUse)
	health := agent.CheckCodexTitleHealth()
	switch health.Status {
	case agent.CodexTitleStatusNormal:
		reportCheck(true, "Codex 会话标题", "正常："+health.Detail)
	case agent.CodexTitleStatusDegraded:
		reportWarning("Codex 会话标题", "降级："+health.Detail)
	default:
		reportCheck(false, "Codex 会话标题", "故障："+health.Detail)
		failures++
	}
	if upstream := agent.FindCodexComputerUseExe(); upstream != "" {
		reportCheck(true, "Codex 上游程序", upstream)
	} else {
		reportCheck(false, "Codex 上游程序", "未找到 codex-computer-use.exe")
		failures++
	}

	reportCheck(true, "推送开关", fmt.Sprintf(
		"opencode=%s codex=%s antigravity=%s devin=%s",
		onOff(!marker.IsOff(paths.OpenCodeMarker)),
		onOff(!marker.IsOff(paths.CodexMarker)),
		onOff(!marker.IsOff(paths.AntigravityMarker)),
		onOff(!marker.IsOff(paths.DevinMarker)),
	))
	if _, err := notify.GetHistory(1, paths.PushLog); err != nil {
		reportCheck(false, "推送日志可读", err.Error())
		failures++
	} else {
		reportCheck(true, "推送日志可读", paths.PushLog)
	}

	fmt.Println()
	if failures > 0 {
		fmt.Printf("检查完成：%d 项失败。\n", failures)
		return 1
	}
	fmt.Println("检查完成：全部通过。")
	return 0
}

func checkCodexReplySupport(cfg config.AppConfig, codexInUse bool) int {
	ctx, cancel := context.WithTimeout(context.Background(), doctorCodexQueueTimeout)
	defer cancel()

	binary, err := reply.CheckCodexQueue(ctx)
	if err != nil {
		if cfg.ReplyEnabled && codexInUse {
			reportCheck(false, "Codex 引用回复", err.Error())
			return 1
		}
		reportWarning("Codex 引用回复", err.Error()+"；仅影响 Codex 引用回复，普通通知仍可用")
		return 0
	}

	reportCheck(true, "Codex 引用回复", binary+"（按线程精确投递）")
	return 0
}

func reportCheck(ok bool, name, detail string) {
	state := "OK"
	if !ok {
		state = "FAIL"
	}
	fmt.Printf("[%-4s] %s", state, name)
	if detail != "" {
		fmt.Printf(" - %s", detail)
	}
	fmt.Println()
}

func reportWarning(name, detail string) {
	fmt.Printf("[%-4s] %s", "WARN", name)
	if detail != "" {
		fmt.Printf(" - %s", detail)
	}
	fmt.Println()
}

func reportHookIntegration(name, path, commandSuffix string) {
	data, err := os.ReadFile(path)
	if err != nil {
		if os.IsNotExist(err) {
			reportWarning(name, "未发现配置文件，按未使用处理")
		} else {
			reportWarning(name, err.Error())
		}
		return
	}
	content := strings.ToLower(string(data))
	if strings.Contains(content, "agent-notify") && strings.Contains(content, strings.ToLower(commandSuffix)) {
		reportCheck(true, name, path)
		return
	}
	reportWarning(name, "未发现 Agent-notify hook，请重新运行 install.ps1")
}

func runToggle(args []string) int {
	flags := flag.NewFlagSet("toggle", flag.ContinueOnError)
	agentName := flags.String("agent", "all", "all、opencode、codex、antigravity 或 devin")
	on := flags.Bool("on", false, "开启")
	off := flags.Bool("off", false, "关闭")
	if err := flags.Parse(args); err != nil {
		return 2
	}
	mode := "Flip"
	if *on {
		mode = "On"
	}
	if *off {
		mode = "Off"
	}
	if err := agent.HandleToggle(*agentName, mode); err != nil {
		fmt.Fprintln(os.Stderr, err)
		return 1
	}
	return 0
}

func runWatch(args []string) int {
	flags := flag.NewFlagSet("watch", flag.ContinueOnError)
	configPath := flags.String("config", "", "Codex config.toml 路径")
	exePath := flags.String("exe", "", "Agent-notify 可执行文件路径")
	if err := flags.Parse(args); err != nil {
		return 2
	}
	if err := agent.HandleWatch(*configPath, *exePath); err != nil {
		fmt.Fprintln(os.Stderr, err)
		return 1
	}
	fmt.Println("Codex notify 配置检查完成。")
	return 0
}

func runHistory(args []string) int {
	flags := flag.NewFlagSet("history", flag.ContinueOnError)
	limit := flags.Int("limit", 20, "返回条数")
	asJSON := flags.Bool("json", false, "以 JSON 输出，便于脚本消费")
	if err := flags.Parse(args); err != nil {
		return 2
	}
	records, err := notify.GetHistory(*limit, "")
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		return 1
	}
	if *asJSON {
		if records == nil {
			records = []notify.HistoryItem{}
		}
		payload, err := json.MarshalIndent(records, "", "  ")
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
			return 1
		}
		fmt.Println(string(payload))
		return 0
	}
	if len(records) == 0 {
		fmt.Println("暂无推送记录。")
		return 0
	}
	fmt.Printf("%-12s  %-9s  %-8s  %s\n", "时间", "来源", "状态", "标题")
	for _, item := range records {
		timestamp := historyTime(item)
		fmt.Printf("%-12s  %-9s  %-8s  %s\n", timestamp, historyAgent(item), item.Status, item.Title)
	}
	return 0
}

func historyTime(item notify.HistoryItem) string {
	if timestamp := item.LocalTime(); !timestamp.IsZero() {
		return timestamp.Format("01-02 15:04")
	}
	return item.Timestamp
}

func historyAgent(item notify.HistoryItem) string {
	if item.Agent == "" {
		return "通用"
	}
	if descriptor, ok := agentmeta.Lookup(item.Agent); ok {
		return descriptor.DisplayName
	}
	return item.Agent
}

func loginStatus(status clawbot.Status) string {
	switch {
	case !status.LoggedIn:
		return "未登录"
	case status.Stale:
		return "登录已失效，请重新扫码"
	case !status.SessionReady:
		return "已登录 · 等待微信消息建立会话"
	default:
		if status.UserHint == "" {
			return "已登录 · 会话就绪"
		}
		return "已登录 · 会话就绪 · " + status.UserHint
	}
}

func sessionStatus(status clawbot.Status) string {
	switch {
	case !status.LoggedIn:
		return "未建立（未登录）"
	case status.Stale:
		return "未建立（登录已失效）"
	case status.SessionReady:
		return "就绪"
	default:
		return "未建立（请先给 ClawBot 发一条微信消息）"
	}
}

func onOff(enabled bool) string {
	if enabled {
		return "开启"
	}
	return "暂停"
}

func emptyAs(value, fallback string) string {
	if strings.TrimSpace(value) == "" {
		return fallback
	}
	return value
}

func installedStatus(installed bool) string {
	if installed {
		return "已安装"
	}
	return "未安装"
}

func fileExists(path string) bool {
	_, err := os.Stat(path)
	return err == nil
}

func codexConfigPath() string {
	if value := strings.TrimSpace(os.Getenv("AGENT_NOTIFY_CODEX_CONFIG")); value != "" {
		return value
	}
	home := strings.TrimSpace(os.Getenv("USERPROFILE"))
	if home == "" {
		if value, err := os.UserHomeDir(); err == nil {
			home = value
		}
	}
	return filepath.Join(home, ".codex", "config.toml")
}

func firstHistory(items []notify.HistoryItem) interface{} {
	if len(items) == 0 {
		return nil
	}
	return items[0]
}

func init() {
	runtime.LockOSThread()
}
