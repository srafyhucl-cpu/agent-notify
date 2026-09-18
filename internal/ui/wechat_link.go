//go:build windows

package ui

// wechatLinkState 把 ClawBot 登录与主动推送会话状态收敛为界面可用的单一状态。
// 四处显示与托盘提醒全部只读这个枚举，避免同一状态在多处各判一次、判法还不一致。
type wechatLinkState int

const (
	// wechatLinkOK 已登录且主动推送会话可用。
	wechatLinkOK wechatLinkState = iota
	// wechatLinkAwaitingFirst 已登录但还没收到过微信消息，属于登录流程里的正常等待。
	wechatLinkAwaitingFirst
	// wechatLinkBroken 曾经建立过会话、之后被服务端回收，属于故障。
	wechatLinkBroken
	// wechatLinkNotLoggedIn 没有可用凭据。
	wechatLinkNotLoggedIn
	// wechatLinkStale 凭据已被服务端判定失效。
	wechatLinkStale
)

// wechatLinkStateFor 是四处显示与托盘提醒共用的唯一判定入口。
func wechatLinkStateFor(loggedIn, stale, sessionReady, everReady bool) wechatLinkState {
	switch {
	case !loggedIn:
		return wechatLinkNotLoggedIn
	case stale:
		return wechatLinkStale
	case sessionReady:
		return wechatLinkOK
	case everReady:
		return wechatLinkBroken
	default:
		return wechatLinkAwaitingFirst
	}
}

// wechatDockLabel 返回悬浮窗底部「微信」入口的文案与是否强调。
// 未登录维持原有红色「微信未连」；登录失效维持现状，只由托盘与设置页提示。
func wechatDockLabel(state wechatLinkState) (string, bool) {
	switch state {
	case wechatLinkNotLoggedIn:
		return "微信未连", true
	case wechatLinkAwaitingFirst:
		return "待发消息", true
	case wechatLinkBroken:
		return "推送已断", true
	default:
		return "微信配置", false
	}
}

// wechatDockAccent 返回底部入口的强调色；0 表示不着色（维持现状）。
func wechatDockAccent(theme ThemePalette, state wechatLinkState) uint32 {
	switch state {
	case wechatLinkNotLoggedIn:
		return theme.AccentDanger
	case wechatLinkAwaitingFirst, wechatLinkBroken:
		return theme.AccentWarning
	default:
		return 0
	}
}

// wechatCardText 返回设置页 ClawBot 卡片的状态文案与指示灯颜色。
func wechatCardText(theme ThemePalette, state wechatLinkState) (string, uint32) {
	switch state {
	case wechatLinkNotLoggedIn:
		return "ClawBot 微信未登录", theme.AccentDanger
	case wechatLinkStale:
		return "ClawBot 微信登录已失效", theme.AccentDanger
	case wechatLinkAwaitingFirst:
		return "等待微信消息", theme.AccentWarning
	case wechatLinkBroken:
		return "主动推送会话已失效", theme.AccentWarning
	default:
		return "ClawBot 微信会话正常", theme.AccentSuccess
	}
}

// wechatLinkCardText 返回微信配置页顶部卡片在某种链路状态下的图标、标题与说明。
func wechatLinkCardText(state wechatLinkState) (icon, title, hint string) {
	switch state {
	case wechatLinkAwaitingFirst:
		return "\uE7BA", "已登录，等待第一条消息", "请在微信中给 ClawBot 发送任意一条消息，用于建立主动推送会话。"
	case wechatLinkBroken:
		return "\uE7BA", "主动推送会话已断开", "任务通知暂时发不出去。请在微信中给 ClawBot 发送任意一条消息即可恢复。"
	case wechatLinkStale:
		return "\uE783", "ClawBot 微信登录已失效", "请点击下方「刷新二维码」重新扫码登录。"
	default:
		return "\uE73E", "ClawBot 微信已成功连接", "主动推送链路正常。任务完成后将自动通过微信发送消息。"
	}
}

// wechatLinkAccent 返回微信配置页卡片的强调色。
func wechatLinkAccent(theme ThemePalette, state wechatLinkState) uint32 {
	switch state {
	case wechatLinkOK:
		return theme.AccentSuccess
	case wechatLinkStale, wechatLinkNotLoggedIn:
		return theme.AccentDanger
	default:
		return theme.AccentWarning
	}
}
