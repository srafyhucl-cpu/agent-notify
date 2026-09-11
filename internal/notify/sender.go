package notify

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"time"

	"linkweixin/internal/config"
)

var reSpaces = regexp.MustCompile(`[\r\n\t]+`)

// NotifyOptions holds arguments for sending notification.
type NotifyOptions struct {
	Title    string
	Summary  string
	MaxChars int
	DryRun   bool
}

// NotifyResult contains execution details of the notification.
type NotifyResult struct {
	PushedChannels []string
	FailedChannels []string
	Status         string
	DryRunPayload  string
}

// SendNotification dispatches notification across configured channels (PushPlus, WeCom, Feishu, DingTalk, Custom).
// It never panics and logs execution record to PushLog.
func SendNotification(opts NotifyOptions) NotifyResult {
	if opts.MaxChars <= 0 {
		opts.MaxChars = 500
	}

	title := opts.Title
	if strings.TrimSpace(title) == "" {
		title = "【AI任务】跑完了"
	}
	if !strings.HasPrefix(title, "【") {
		title = "【AI任务】" + title
	}

	summary := opts.Summary
	if strings.TrimSpace(summary) == "" {
		summary = fmt.Sprintf("任务完成，上线查看详情。[%s]", time.Now().Format("01-02 15:04:05"))
	}

	rendered := FormatNotifySummary(summary, opts.MaxChars)

	cfg := config.LoadConfig("")
	paths := config.GetPaths()

	token := ""
	if cfg.Channels.PushPlus.Enabled && strings.TrimSpace(cfg.Channels.PushPlus.Token) != "" {
		token = strings.TrimSpace(cfg.Channels.PushPlus.Token)
	} else if envToken := os.Getenv("PUSHPLUS_TOKEN"); strings.TrimSpace(envToken) != "" {
		token = strings.TrimSpace(envToken)
	}

	// Build PushPlus payload map
	pushPlusPayload := map[string]interface{}{
		"title":    title,
		"content":  rendered,
		"template": "html",
	}
	if token != "" {
		pushPlusPayload["token"] = token
	}

	if opts.DryRun {
		shownPayload := make(map[string]interface{})
		for k, v := range pushPlusPayload {
			shownPayload[k] = v
		}
		if _, ok := shownPayload["token"]; ok {
			shownPayload["token"] = "****"
		}
		data, _ := json.Marshal(shownPayload)
		return NotifyResult{
			Status:        "DryRun",
			DryRunPayload: string(data),
		}
	}

	var pushed []string
	var failed []string

	// 1. PushPlus
	if token != "" && cfg.Channels.PushPlus.Enabled {
		bodyBytes, _ := json.Marshal(pushPlusPayload)
		client := &http.Client{Timeout: 20 * time.Second}
		req, err := http.NewRequest("POST", "https://www.pushplus.plus/send", bytes.NewBuffer(bodyBytes))
		if err == nil {
			req.Header.Set("Content-Type", "application/json; charset=utf-8")
			resp, err := client.Do(req)
			if err != nil {
				failed = append(failed, "PushPlus")
				_, _ = fmt.Fprintf(os.Stderr, "[notify-ai] PushPlus 失败: %v\n", err)
			} else {
				respBytes, _ := io.ReadAll(resp.Body)
				_ = resp.Body.Close()
				var ppResp struct {
					Code int    `json:"code"`
					Msg  string `json:"msg"`
				}
				_ = json.Unmarshal(respBytes, &ppResp)
				if ppResp.Code == 200 {
					pushed = append(pushed, "PushPlus")
				} else {
					failed = append(failed, "PushPlus")
					_, _ = fmt.Fprintf(os.Stderr, "[notify-ai] PushPlus 返回 code=%d msg=%s\n", ppResp.Code, ppResp.Msg)
				}
			}
		} else {
			failed = append(failed, "PushPlus")
		}
	} else if token == "" && cfg.Channels.PushPlus.Enabled {
		_, _ = fmt.Fprintln(os.Stderr, "[notify-ai] PUSHPLUS_TOKEN 为空，跳过 PushPlus 通道。")
	}

	// 2. 企业微信 Webhook
	wecomUrl := strings.TrimSpace(cfg.Channels.WeCom.Webhook)
	if cfg.Channels.WeCom.Enabled && wecomUrl != "" {
		wecomBody, _ := json.Marshal(map[string]interface{}{
			"msgtype": "markdown",
			"markdown": map[string]interface{}{
				"content": fmt.Sprintf("### %s\r\n\r\n%s", title, summary),
			},
		})
		client := &http.Client{Timeout: 15 * time.Second}
		resp, err := client.Post(wecomUrl, "application/json; charset=utf-8", bytes.NewBuffer(wecomBody))
		if err == nil {
			respBytes, _ := io.ReadAll(resp.Body)
			_ = resp.Body.Close()
			var wxResp struct {
				ErrCode int `json:"errcode"`
			}
			_ = json.Unmarshal(respBytes, &wxResp)
			if wxResp.ErrCode == 0 {
				pushed = append(pushed, "企业微信")
			} else {
				failed = append(failed, "企业微信")
			}
		} else {
			failed = append(failed, "企业微信")
			_, _ = fmt.Fprintf(os.Stderr, "[notify-ai] 企业微信 Webhook 失败: %v\n", err)
		}
	}

	// 3. 飞书 Webhook
	feishuUrl := strings.TrimSpace(cfg.Channels.Feishu.Webhook)
	if cfg.Channels.Feishu.Enabled && feishuUrl != "" {
		feishuBody, _ := json.Marshal(map[string]interface{}{
			"msg_type": "interactive",
			"card": map[string]interface{}{
				"header": map[string]interface{}{
					"title":    map[string]interface{}{"tag": "plain_text", "content": title},
					"template": "blue",
				},
				"elements": []interface{}{
					map[string]interface{}{
						"tag":  "div",
						"text": map[string]interface{}{"tag": "lark_md", "content": summary},
					},
				},
			},
		})
		client := &http.Client{Timeout: 15 * time.Second}
		resp, err := client.Post(feishuUrl, "application/json; charset=utf-8", bytes.NewBuffer(feishuBody))
		if err == nil {
			respBytes, _ := io.ReadAll(resp.Body)
			_ = resp.Body.Close()
			var fsResp struct {
				Code int `json:"code"`
			}
			_ = json.Unmarshal(respBytes, &fsResp)
			if fsResp.Code == 0 {
				pushed = append(pushed, "飞书")
			} else {
				failed = append(failed, "飞书")
			}
		} else {
			failed = append(failed, "飞书")
			_, _ = fmt.Fprintf(os.Stderr, "[notify-ai] 飞书 Webhook 失败: %v\n", err)
		}
	}

	// 4. 钉钉 Webhook
	dingUrl := strings.TrimSpace(cfg.Channels.DingTalk.Webhook)
	if cfg.Channels.DingTalk.Enabled && dingUrl != "" {
		dingBody, _ := json.Marshal(map[string]interface{}{
			"msgtype": "markdown",
			"markdown": map[string]interface{}{
				"title": title,
				"text":  fmt.Sprintf("### %s\r\n\r\n%s", title, summary),
			},
		})
		client := &http.Client{Timeout: 15 * time.Second}
		resp, err := client.Post(dingUrl, "application/json; charset=utf-8", bytes.NewBuffer(dingBody))
		if err == nil {
			respBytes, _ := io.ReadAll(resp.Body)
			_ = resp.Body.Close()
			var ddResp struct {
				ErrCode int `json:"errcode"`
			}
			_ = json.Unmarshal(respBytes, &ddResp)
			if ddResp.ErrCode == 0 {
				pushed = append(pushed, "钉钉")
			} else {
				failed = append(failed, "钉钉")
			}
		} else {
			failed = append(failed, "钉钉")
			_, _ = fmt.Fprintf(os.Stderr, "[notify-ai] 钉钉 Webhook 失败: %v\n", err)
		}
	}

	// 5. 自定义 Webhook
	customUrl := strings.TrimSpace(cfg.Channels.Custom.Webhook)
	if cfg.Channels.Custom.Enabled && customUrl != "" {
		customBody, _ := json.Marshal(map[string]interface{}{
			"title":     title,
			"content":   summary,
			"rendered":  rendered,
			"timestamp": time.Now().Format(time.RFC3339),
		})
		client := &http.Client{Timeout: 15 * time.Second}
		_, err := client.Post(customUrl, "application/json; charset=utf-8", bytes.NewBuffer(customBody))
		if err == nil {
			pushed = append(pushed, "自定义Webhook")
		} else {
			failed = append(failed, "自定义Webhook")
			_, _ = fmt.Fprintf(os.Stderr, "[notify-ai] 自定义 Webhook 失败: %v\n", err)
		}
	}

	// 日志落盘（保持时间戳在首列与管道符格式）
	chanText := "无通道投递"
	if len(pushed) > 0 {
		chanText = strings.Join(pushed, ",")
	}
	statusText := "未发送"
	if len(failed) == 0 && len(pushed) > 0 {
		statusText = "成功"
	} else if len(pushed) > 0 {
		statusText = "部分成功"
	}

	shortSummary := strings.TrimSpace(reSpaces.ReplaceAllString(summary, " "))
	runes := []rune(shortSummary)
	if len(runes) > 120 {
		shortSummary = string(runes[:120]) + "..."
	}

	nowIso := time.Now().UTC().Format("2006-01-02T15:04:05.000Z")
	logEntry := fmt.Sprintf("%s push title=%s | channels=%s | status=%s | summary=%s\r\n", nowIso, title, chanText, statusText, shortSummary)

	logPath := paths.PushLog
	_ = os.MkdirAll(filepath.Dir(logPath), 0755)
	f, err := os.OpenFile(logPath, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0644)
	if err == nil {
		_, _ = f.WriteString(logEntry)
		_ = f.Close()
	}

	return NotifyResult{
		PushedChannels: pushed,
		FailedChannels: failed,
		Status:         statusText,
	}
}
