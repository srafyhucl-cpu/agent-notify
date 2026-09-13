package notify

import "testing"

func TestParseProtocolBlocksHeartbeatPolicy(t *testing.T) {
	tests := []struct {
		name      string
		input     string
		body      string
		silent    bool
		heartbeat HeartbeatState
	}{
		{
			name:      "normal heartbeat stays silent",
			input:     "<heartbeat>\n<automation_id>agent-notify</automation_id>\n<decision>DONT_NOTIFY</decision>\n<message>一切正常</message>\n</heartbeat>",
			silent:    true,
			heartbeat: HeartbeatNormal,
		},
		{
			name:      "abnormal heartbeat keeps the message",
			input:     "<heartbeat>\n<decision>NOTIFY</decision>\n<message>推送失败 3 次</message>\n</heartbeat>",
			body:      "推送失败 3 次",
			heartbeat: HeartbeatAbnormal,
		},
		{
			name:      "unknown heartbeat decision is pushed",
			input:     "<heartbeat>\n<message>状态未知</message>\n</heartbeat>",
			body:      "状态未知",
			heartbeat: HeartbeatUnknown,
		},
		{
			name:      "abnormal heartbeat without details is still pushed",
			input:     "<heartbeat><decision>NOTIFY</decision></heartbeat>",
			body:      heartbeatAbnormalFallbackBody,
			heartbeat: HeartbeatAbnormal,
		},
		{
			name:      "unknown heartbeat without details is still pushed",
			input:     "<heartbeat></heartbeat>",
			body:      heartbeatUnknownFallbackBody,
			heartbeat: HeartbeatUnknown,
		},
		{
			name:      "upper case tags and decision value",
			input:     "<HEARTBEAT><DECISION>dont_notify</DECISION></HEARTBEAT>",
			silent:    true,
			heartbeat: HeartbeatNormal,
		},
		{
			name:      "top level dont notify wins over body",
			input:     "正文内容\n<decision>DONT_NOTIFY</decision>",
			silent:    true,
			heartbeat: HeartbeatNone,
		},
		{
			name:      "body outside heartbeat is kept",
			input:     "开始\n<automation_id>x</automation_id>正文\n结束",
			body:      "开始\n正文\n结束",
			heartbeat: HeartbeatNone,
		},
		{
			name:      "only protocol blocks stays silent",
			input:     "<automation_id>x</automation_id>\n<decision>NOTIFY</decision>",
			silent:    true,
			heartbeat: HeartbeatNone,
		},
		{
			name:      "unknown tags are preserved",
			input:     "前<foo bar=\"1\">内容</foo>后",
			body:      "前<foo bar=\"1\">内容</foo>后",
			heartbeat: HeartbeatNone,
		},
		{
			name:      "unbalanced heartbeat stays visible",
			input:     "<heartbeat><decision>DONT_NOTIFY</decision> 未闭合",
			body:      "<heartbeat> 未闭合",
			heartbeat: HeartbeatNone,
		},
		{
			name:      "multiple heartbeats keep the abnormal one",
			input:     "<heartbeat><decision>DONT_NOTIFY</decision><message>正常</message></heartbeat>\n<heartbeat><decision>NOTIFY</decision><message>异常</message></heartbeat>",
			body:      "异常",
			heartbeat: HeartbeatAbnormal,
		},
		{
			name:      "dont notify wins over an abnormal heartbeat",
			input:     "<heartbeat><decision>NOTIFY</decision><message>异常</message></heartbeat>\n<decision>DONT_NOTIFY</decision>",
			silent:    true,
			heartbeat: HeartbeatAbnormal,
		},
	}

	for _, testCase := range tests {
		t.Run(testCase.name, func(t *testing.T) {
			result := ParseProtocolBlocks(testCase.input)
			if result.Silent != testCase.silent {
				t.Fatalf("Silent = %v, want %v (result %#v)", result.Silent, testCase.silent, result)
			}
			if result.Heartbeat != testCase.heartbeat {
				t.Fatalf("Heartbeat = %q, want %q", result.Heartbeat, testCase.heartbeat)
			}
			if !testCase.silent && result.Body != testCase.body {
				t.Fatalf("Body = %q, want %q", result.Body, testCase.body)
			}
			if testCase.silent && result.Reason == "" {
				t.Fatalf("silent result has no reason: %#v", result)
			}
		})
	}
}
