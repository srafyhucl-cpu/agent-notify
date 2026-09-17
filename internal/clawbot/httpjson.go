package clawbot

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"net/http"
)

// jsonRequestStage 标识 JSON 请求失败的阶段，让调用方按各自原有文案包装错误。
type jsonRequestStage int

const (
	jsonStageMarshal jsonRequestStage = iota
	jsonStageNewRequest
	jsonStageDo
	jsonStageReadBody
	jsonStageDecode
)

// jsonRequestOptions 描述各调用方的差异点：鉴权头 / 额外头、debug 钩子、
// 非 2xx 的错误类型与错误包装文案。相同的「构造请求 → 发送 → 读响应体 →
// 判定状态码 → 解码 JSON」骨架由 doJSONRequest 复用。
type jsonRequestOptions struct {
	// Headers 是本次请求要设置的头部，可为空。
	Headers map[string]string
	// OnResponse 在状态码为 2xx 且解码之前调用，body 为原始响应体，可为空。
	OnResponse func(status int, body []byte)
	// StatusError 构造非 2xx 错误；为空时返回 *httpStatusError。
	StatusError func(status int, body []byte) error
	// WrapError 包装构造请求、发送、读取与解码阶段的错误；为空时原样返回。
	WrapError func(stage jsonRequestStage, err error) error
}

// doJSONRequest 执行一次 JSON 请求，成功时把响应体解码到 result。
// body 为 nil 时不发送请求体，也不由本函数补 Content-Type。
func doJSONRequest(ctx context.Context, client *http.Client, method, endpoint string, body any, result any, options jsonRequestOptions) error {
	var reader io.Reader
	if body != nil {
		data, err := json.Marshal(body)
		if err != nil {
			return wrapJSONError(options.WrapError, jsonStageMarshal, err)
		}
		reader = bytes.NewReader(data)
	}

	req, err := http.NewRequestWithContext(ctx, method, endpoint, reader)
	if err != nil {
		return wrapJSONError(options.WrapError, jsonStageNewRequest, err)
	}
	for key, value := range options.Headers {
		req.Header.Set(key, value)
	}

	resp, err := client.Do(req)
	if err != nil {
		return wrapJSONError(options.WrapError, jsonStageDo, err)
	}
	defer resp.Body.Close()

	respData, err := readResponseBody(resp.Body)
	if err != nil {
		return wrapJSONError(options.WrapError, jsonStageReadBody, err)
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		if options.StatusError != nil {
			return options.StatusError(resp.StatusCode, respData)
		}
		return &httpStatusError{status: resp.StatusCode, body: string(respData)}
	}
	if options.OnResponse != nil {
		options.OnResponse(resp.StatusCode, respData)
	}
	if err := json.Unmarshal(respData, result); err != nil {
		return wrapJSONError(options.WrapError, jsonStageDecode, err)
	}
	return nil
}

func wrapJSONError(wrap func(stage jsonRequestStage, err error) error, stage jsonRequestStage, err error) error {
	if wrap == nil {
		return err
	}
	return wrap(stage, err)
}
