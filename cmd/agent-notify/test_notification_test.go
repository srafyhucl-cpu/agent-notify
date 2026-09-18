package main

import (
	"strings"
	"testing"
)

// 自检通知的 Agent 是 "test" 且没有会话 ID，不会写入引用路由；
// 文案不能声称可以用来验证引用续聊，否则用户引用后只会收到「无法续聊」。
func TestTestNotificationBodyDoesNotPromiseReply(t *testing.T) {
	body := testNotificationBody()
	if strings.Contains(body, "可验证反向引用回复路由") {
		t.Fatal("自检通知不得声称可验证引用续聊：它不携带会话 ID，不会写入引用路由")
	}
	if !strings.Contains(body, "不能用于引用续聊") {
		t.Fatal("自检通知必须明确说明不能用于引用续聊")
	}
}
