package main

import (
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io/fs"
	"os"
	"strings"

	"github.com/srafyhucl-cpu/agent-notify/internal/clawbot"
	"github.com/srafyhucl-cpu/agent-notify/internal/config"
	"github.com/srafyhucl-cpu/agent-notify/internal/reply"
)

const (
	replyCheckExitOK         = 0
	replyCheckExitFailed     = 1
	replyCheckExitIncomplete = 2

	maxReplyCheckQuotesShown = 20
)

type replyCheckOutput struct {
	Path         string                `json:"path"`
	ReplyEnabled bool                  `json:"replyEnabled"`
	Error        string                `json:"error,omitempty"`
	Gate         reply.ReplyGateReport `json:"gate"`
}

// runReplyCheck reports whether quoted WeChat replies carry an identifier that
// matches a recorded notification. It only reads local diagnostics and never
// changes the reply toggle.
func runReplyCheck(args []string) int {
	flags := flag.NewFlagSet("reply-check", flag.ContinueOnError)
	jsonOut := flags.Bool("json", false, "以 JSON 输出")
	if err := flags.Parse(args); err != nil {
		return replyCheckExitIncomplete
	}

	paths := config.GetPaths()
	cfg, cfgErr := config.LoadConfig("")
	if cfgErr != nil {
		output := replyCheckOutput{Path: paths.ClawbotDebugLog}
		return replyCheckFailure(output, fmt.Sprintf("读取配置失败：%v", cfgErr), *jsonOut)
	}

	output := replyCheckOutput{Path: paths.ClawbotDebugLog, ReplyEnabled: cfg.ReplyEnabled}
	entries, err := clawbot.ReadClawbotDebugLog(paths.ClawbotDebugLog)
	if err != nil {
		if errors.Is(err, fs.ErrNotExist) {
			output.Gate = reply.EvaluateReplyGate(nil, nil)
			if *jsonOut {
				writeReplyCheckJSON(output)
			} else {
				printReplyCheckHeader(output.Path)
				reportWarning("协议诊断", "尚未生成日志，请设置 AGENT_NOTIFY_CLAWBOT_DEBUG=1 并重启悬浮窗后重现流程")
				fmt.Println("结论：证据不足，暂不能开启“引用回复”。")
			}
			return replyCheckExitIncomplete
		}
		return replyCheckFailure(output, fmt.Sprintf("读取协议诊断失败：%v", err), *jsonOut)
	}

	resolver, recordedSends, accountScope, resolverErr := replyGateResolver()
	if resolverErr != nil {
		return replyCheckFailure(output, fmt.Sprintf("无法读取引用路由账户范围：%v", resolverErr), *jsonOut)
	}
	output.Gate = reply.EvaluateReplyGateForAccountWithSends(entries, accountScope, recordedSends, resolver)
	if *jsonOut {
		writeReplyCheckJSON(output)
	} else {
		printReplyCheckReport(output)
	}
	switch output.Gate.Status {
	case reply.ReplyGatePassed:
		return replyCheckExitOK
	case reply.ReplyGateFailed:
		return replyCheckExitFailed
	default:
		return replyCheckExitIncomplete
	}
}

func replyCheckFailure(output replyCheckOutput, message string, jsonOut bool) int {
	output.Error = message
	output.Gate = reply.ReplyGateReport{
		Status: reply.ReplyGateFailed,
		Sends:  []reply.ReplyGateSend{},
		Quotes: []reply.ReplyGateQuote{},
	}
	if jsonOut {
		writeReplyCheckJSON(output)
	} else {
		printReplyCheckHeader(output.Path)
		reportWarning("校验失败", message)
		fmt.Println("结论：P0 未通过，暂不能开启“引用回复”。")
	}
	return replyCheckExitFailed
}

func replyGateResolver() (reply.RouteResolver, []reply.RecordedSend, string, error) {
	credentials, err := clawbot.LoadCredentials()
	if err != nil {
		return nil, nil, "", err
	}
	store := reply.NewRouteStore("")
	routes, err := store.ListActive(credentials.ILinkBotID, credentials.ILinkUserID)
	if err != nil {
		return nil, nil, "", err
	}
	accountScope := clawbot.AccountScope(credentials.ILinkBotID, credentials.ILinkUserID)
	recordedSends := make([]reply.RecordedSend, 0, len(routes))
	for _, route := range routes {
		recordedSends = append(recordedSends, reply.RecordedSend{
			MessageID:    route.MessageID,
			ClientID:     route.ClientID,
			Agent:        route.Agent,
			SessionID:    route.SessionID,
			AccountScope: accountScope,
		})
	}
	resolver := func(referencedID string) (reply.Route, error) {
		return store.Find(credentials.ILinkBotID, credentials.ILinkUserID, referencedID, referencedID)
	}
	return resolver, recordedSends, accountScope, nil
}

func printReplyCheckReport(output replyCheckOutput) {
	printReplyCheckHeader(output.Path)
	gate := output.Gate

	platformSends := 0
	for _, send := range gate.Sends {
		if send.MessageID != "" {
			platformSends++
		}
	}
	sendDetail := fmt.Sprintf("%d 条（含平台消息 ID %d 条，仅客户端 ID %d 条）",
		len(gate.Sends), platformSends, len(gate.Sends)-platformSends)
	debugSends := 0
	routeSends := 0
	for _, send := range gate.Sends {
		switch send.Source {
		case reply.ReplyGateSendDebug:
			debugSends++
		case reply.ReplyGateSendRoute:
			routeSends++
		}
	}
	sendDetail += fmt.Sprintf("；协议响应 %d 条，本地路由 %d 条", debugSends, routeSends)
	if len(gate.Sends) == 0 {
		reportWarning("发送记录", sendDetail)
	} else {
		reportCheck(true, "发送记录", sendDetail)
	}

	for index, quote := range gate.Quotes {
		if index == maxReplyCheckQuotesShown {
			fmt.Printf("  …另有 %d 条引用未显示\n", len(gate.Quotes)-maxReplyCheckQuotesShown)
			break
		}
		printReplyCheckQuote(quote)
	}

	if len(gate.Quotes) > 0 {
		detail := fmt.Sprintf("%d 条，ID 精确匹配 %d 条", len(gate.Quotes), gate.MatchedQuotes)
		switch {
		case gate.FailedQuotes > 0:
			reportCheck(false, "引用样本", fmt.Sprintf("%s，未匹配 %d 条", detail, gate.FailedQuotes))
		case gate.RouteFailures > 0:
			reportCheck(false, "引用样本", fmt.Sprintf("%s，路由未解析 %d 条", detail, gate.RouteFailures))
		default:
			reportCheck(true, "引用样本", detail)
		}
	}

	if gate.IgnoredSends > 0 || gate.IgnoredQuotes > 0 {
		reportWarning("账号与私聊隔离", fmt.Sprintf(
			"已忽略当前账号之外、非私聊或旧版无范围证据：发送 %d 条，引用 %d 条", gate.IgnoredSends, gate.IgnoredQuotes))
	}

	for _, quote := range gate.Quotes {
		if quote.MatchedID != "" && quote.RouteError != "" {
			reportWarning("路由解析", fmt.Sprintf("引用 %s 未解析到会话：%s", quote.MatchedID, quote.RouteError))
		}
	}
	if output.ReplyEnabled {
		reportWarning("引用回复开关", "当前已开启；本命令只读，不会替你关闭开关")
	}

	switch gate.Status {
	case reply.ReplyGatePassed:
		fmt.Println("结论：P0 对照通过，引用 ID 能精确对应发送记录，可以开启“引用回复”。")
	case reply.ReplyGateFailed:
		fmt.Println("结论：P0 未通过，存在无法对应发送记录或无法解析会话的引用；请勿开启“引用回复”，并把上面的 ID 提供给开发者。")
	case reply.ReplyGateAwaitingQuote:
		fmt.Println("结论：已有发送记录但还没有引用样本；请在微信中引用该通知并回复一句话后重跑本命令。")
	default:
		if gate.IgnoredSends > 0 || gate.IgnoredQuotes > 0 {
			fmt.Println("结论：当前账号尚无可用发送记录；旧版、其他账号或非私聊样本已忽略。请重启最新悬浮窗后重新发送通知。")
		} else {
			fmt.Println("结论：尚无发送记录；请设置 AGENT_NOTIFY_CLAWBOT_DEBUG=1、重启悬浮窗并发送一条通知。")
		}
	}
}

func printReplyCheckQuote(quote reply.ReplyGateQuote) {
	label := "未匹配"
	switch quote.Match {
	case reply.ReplyGateMatchPlatformID:
		label = "匹配平台消息 ID"
	case reply.ReplyGateMatchClientID:
		label = "匹配客户端 ID（平台未返回消息 ID）"
	}
	line := fmt.Sprintf("  引用 %s -> %s（%s）", describeQuoteMessage(quote), strings.Join(quote.ReferencedIDs, "、"), label)
	if quote.ReferenceError != "" {
		line += "，" + quote.ReferenceError
	}
	if quote.RouteAgent != "" || quote.RouteSession != "" {
		line += fmt.Sprintf("，路由 %s/%s", quote.RouteAgent, quote.RouteSession)
	}
	fmt.Println(line)
}

func describeQuoteMessage(quote reply.ReplyGateQuote) string {
	if quote.MessageID == "" {
		return "(未返回 msg_id)"
	}
	return quote.MessageID
}

func writeReplyCheckJSON(output replyCheckOutput) {
	data, err := json.MarshalIndent(output, "", "  ")
	if err != nil {
		fmt.Fprintf(os.Stderr, "编码输出失败：%v\n", err)
		return
	}
	fmt.Println(string(data))
}

func printReplyCheckHeader(path string) {
	fmt.Println("AgentNotify 引用回复 P0 校验（只读）")
	fmt.Printf("诊断日志：%s\n", path)
}
