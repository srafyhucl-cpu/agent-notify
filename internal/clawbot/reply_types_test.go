package clawbot

import (
	"encoding/json"
	"testing"
)

func TestInboundReplyReferenceParsing(t *testing.T) {
	var message InboundMessage
	raw := `{
		"msg_id": 1001,
		"from_user_id": "user-1",
		"message_type": 1,
		"item_list": [
			{"type": 1, "text_item": {"text": "继续处理"}},
			{"type": 3, "ref_msg": {"message_item": {"msg_id": "platform-1", "text_item": {"text": "原通知"}}}}
		]
	}`
	if err := json.Unmarshal([]byte(raw), &message); err != nil {
		t.Fatalf("Unmarshal: %v", err)
	}
	if message.PlatformMessageID() != "1001" {
		t.Fatalf("PlatformMessageID = %q", message.PlatformMessageID())
	}
	if !message.HasReference() {
		t.Fatal("HasReference = false")
	}
	if message.ReferencedMessageID() != "platform-1" {
		t.Fatalf("ReferencedMessageID = %q", message.ReferencedMessageID())
	}
	if message.ReferencedText() != "原通知" {
		t.Fatalf("ReferencedText = %q", message.ReferencedText())
	}
	if message.Text() != "继续处理" {
		t.Fatalf("Text = %q", message.Text())
	}
}

func TestInboundMessageIDFallbacks(t *testing.T) {
	tests := []struct {
		name string
		raw  string
		want string
	}{
		{
			name: "top-level message_id",
			raw:  `{"message_id":7504585872203029768}`,
			want: "7504585872203029768",
		},
		{
			name: "nested item msg_id",
			raw:  `{"item_list":[{"msg_id":"v1:nested"}]}`,
			want: "v1:nested",
		},
		{
			name: "top-level msg_id has priority",
			raw:  `{"msg_id":"top","message_id":123,"item_list":[{"msg_id":"nested"}]}`,
			want: "top",
		},
	}

	for _, testCase := range tests {
		t.Run(testCase.name, func(t *testing.T) {
			var message InboundMessage
			if err := json.Unmarshal([]byte(testCase.raw), &message); err != nil {
				t.Fatalf("Unmarshal: %v", err)
			}
			if got := message.PlatformMessageID(); got != testCase.want {
				t.Fatalf("PlatformMessageID = %q, want %q", got, testCase.want)
			}
		})
	}
}

func TestInboundReplyReferenceAlternateFields(t *testing.T) {
	for name, raw := range map[string]string{
		"top-level":  `{"referenced_msg_id":"platform-2"}`,
		"ref msg":    `{"item_list":[{"ref_msg":{"msg_id":"platform-3"}}]}`,
		"referenced": `{"item_list":[{"ref_msg":{"referenced_msg_id":"platform-4"}}]}`,
	} {
		t.Run(name, func(t *testing.T) {
			var message InboundMessage
			if err := json.Unmarshal([]byte(raw), &message); err != nil {
				t.Fatalf("Unmarshal: %v", err)
			}
			if message.ReferencedMessageID() == "" {
				t.Fatalf("no reference ID parsed from %s", raw)
			}
		})
	}
}

func TestSendMessageResponseIdentifiers(t *testing.T) {
	var response sendMessageResponse
	if err := json.Unmarshal([]byte(`{"ret":0,"message_id":"server-1","client_id":"client-1"}`), &response); err != nil {
		t.Fatal(err)
	}
	result := response.sendResult("fallback")
	if result.MessageID != "server-1" || result.ClientID != "fallback" {
		t.Fatalf("result = %#v", result)
	}

	if result := response.sendResult(""); result.MessageID != "server-1" || result.ClientID != "client-1" {
		t.Fatalf("response client fallback = %#v", result)
	}

	var nested sendMessageResponse
	if err := json.Unmarshal([]byte(`{"ret":0,"data":{"msg_id":1234}}`), &nested); err != nil {
		t.Fatal(err)
	}
	result = nested.sendResult("fallback")
	if result.MessageID != "1234" || result.ClientID != "fallback" {
		t.Fatalf("nested result = %#v", result)
	}

	var scalar sendMessageResponse
	if err := json.Unmarshal([]byte(`{"ret":0,"msg":5678}`), &scalar); err != nil {
		t.Fatal(err)
	}
	result = scalar.sendResult("fallback")
	if result.MessageID != "5678" || result.ClientID != "fallback" {
		t.Fatalf("scalar result = %#v", result)
	}

	var nestedItem sendMessageResponse
	if err := json.Unmarshal([]byte(`{"ret":0,"msg":{"item_list":[{"msg_id":9012}]}}`), &nestedItem); err != nil {
		t.Fatal(err)
	}
	result = nestedItem.sendResult("fallback")
	if result.MessageID != "9012" || result.ClientID != "fallback" {
		t.Fatalf("nested item result = %#v", result)
	}

	var itemArray sendMessageResponse
	if err := json.Unmarshal([]byte(`{"ret":0,"data":[{"msg_id":3456}]}`), &itemArray); err != nil {
		t.Fatal(err)
	}
	result = itemArray.sendResult("fallback")
	if result.MessageID != "3456" || result.ClientID != "fallback" {
		t.Fatalf("item array result = %#v", result)
	}
}

func TestInboundReplyReferenceConflictingIDsAreAmbiguous(t *testing.T) {
	var message InboundMessage
	raw := `{
		"referenced_msg_id": "platform-top",
		"item_list": [
			{"ref_msg": {"message_item": {"msg_id": "platform-nested"}}}
		]
	}`
	if err := json.Unmarshal([]byte(raw), &message); err != nil {
		t.Fatal(err)
	}
	if ids := message.ReferencedMessageIDs(); len(ids) != 2 {
		t.Fatalf("ReferencedMessageIDs = %#v, want 2 distinct values", ids)
	}
	if got := message.ReferencedMessageID(); got != "" {
		t.Fatalf("ReferencedMessageID = %q, want empty for conflicting IDs", got)
	}
}

func TestInboundReplyReferenceDuplicateIDsAreDeduplicated(t *testing.T) {
	var message InboundMessage
	raw := `{
		"referenced_msg_id": 1001,
		"item_list": [
			{"ref_msg": {"message_item": {"msg_id": "1001"}}}
		]
	}`
	if err := json.Unmarshal([]byte(raw), &message); err != nil {
		t.Fatal(err)
	}
	if got := message.ReferencedMessageID(); got != "1001" {
		t.Fatalf("ReferencedMessageID = %q, want 1001", got)
	}
}

func TestFlexibleStringRejectsNonIdentifierJSONTypes(t *testing.T) {
	for _, raw := range []string{"true", "false", "{}", "[]"} {
		t.Run(raw, func(t *testing.T) {
			var value FlexibleString
			if err := json.Unmarshal([]byte(raw), &value); err == nil {
				t.Fatalf("Unmarshal(%s) = %q, want error", raw, value)
			}
		})
	}
}
