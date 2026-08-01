package main

import (
	"bytes"
	"crypto/tls"
	"encoding/json"
	"io"
	"net"
	"net/http"
	"net/url"
	"os"
	"strconv"
	"testing"
)

func TestInjectWeChatHook(t *testing.T) {
	output := make(chan message, 2)
	s := &server{out: output, resources: map[string]resource{}}
	req := &http.Request{URL: &url.URL{Scheme: "https", Host: "channels.weixin.qq.com", Path: "/web/pages/feed"}}
	resp := &http.Response{Header: http.Header{"Content-Type": []string{"text/html"}}, Body: io.NopCloser(bytes.NewBufferString(`<html><head></head><body></body></html>`)), Request: req}
	updated := s.injectWeChatHook(resp, req)
	body, err := io.ReadAll(updated.Body)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Contains(body, []byte(`src="/__cobalt/hook.js"`)) {
		t.Fatalf("injected callback missing: %s", body)
	}
}

func TestInjectWeChatBridgeMethod(t *testing.T) {
	output := make(chan message, 2)
	s := &server{out: output, resources: map[string]resource{}}
	req := &http.Request{URL: &url.URL{Scheme: "https", Host: "res.wx.qq.com", Path: "/web/web-finder/res/js/virtual_svg-icons-register.publish.js"}}
	source := `class A{async finderGetCommentDetail(e){return await this.call(e)}async finderOther(){return 1}}`
	resp := &http.Response{Header: http.Header{"Content-Type": []string{"application/javascript"}}, Body: io.NopCloser(bytes.NewBufferString(source)), Request: req}
	updated := s.injectWeChatHook(resp, req)
	body, err := io.ReadAll(updated.Body)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Contains(body, []byte("window.__cobalt_report")) || !bytes.Contains(body, []byte("return __cobalt_result")) {
		t.Fatalf("bridge method was not patched: %s", body)
	}
}

func TestHTTPSProxyCapturesWeChatCallback(t *testing.T) {
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	port := listener.Addr().(*net.TCPAddr).Port
	_ = listener.Close()
	dataDir, err := os.MkdirTemp("", "cobalt-sniffer-test-")
	if err != nil {
		t.Fatal(err)
	}
	defer os.RemoveAll(dataDir)
	output := make(chan message, 4)
	s := &server{out: output, resources: map[string]resource{}, dataDir: dataDir}
	if err := s.start(port, ""); err != nil {
		t.Fatal(err)
	}
	defer s.httpServer.Close()
	<-output // startup status
	proxyURL, _ := url.Parse("http://127.0.0.1:" + strconv.Itoa(port))
	client := &http.Client{Transport: &http.Transport{Proxy: http.ProxyURL(proxyURL), TLSClientConfig: &tls.Config{InsecureSkipVerify: true}}}
	hookResponse, err := client.Get("https://channels.weixin.qq.com/__cobalt/hook.js")
	if err != nil {
		t.Fatal(err)
	}
	hookBody, err := io.ReadAll(hookResponse.Body)
	_ = hookResponse.Body.Close()
	if err != nil || !bytes.Contains(hookBody, []byte("window.__cobalt_report")) {
		t.Fatalf("same-origin hook was not served: %s", hookBody)
	}
	response, err := client.Post("https://channels.weixin.qq.com/__cobalt/wechat", "application/json", bytes.NewBufferString(`{"description":"HTTPS fixture","media":[{"url":"https://finder.video.qq.com/video.mp4","urlToken":"?token=abc","decodeKey":"AQID"}]}`))
	if err != nil {
		t.Fatal(err)
	}
	_ = response.Body.Close()
	resourceEvent := <-output
	if resourceEvent.Type != "resource" {
		t.Fatalf("expected resource event, got %#v", resourceEvent)
	}
	if len(s.resources) != 1 {
		t.Fatal("expected captured callback resource")
	}
}

func TestCaptureWeChatCallbackKeepsDecodeKeyPrivate(t *testing.T) {
	output := make(chan message, 1)
	s := &server{out: output, resources: map[string]resource{}}
	req := &http.Request{Method: http.MethodPost, URL: &url.URL{Scheme: "https", Host: "channels.weixin.qq.com", Path: "/__cobalt/wechat"}, Header: http.Header{"User-Agent": []string{"fixture"}}, Body: io.NopCloser(bytes.NewBufferString(`{"description":"fixture video","media":[{"url":"https://finder.video.qq.com/file.mp4?x=1","urlToken":"&token=2","fileSize":"42","decodeKey":"AQID"}]}`))}
	s.captureWeChatCallback(req)
	resourceEvent := <-output
	public := resourceEvent.Data.(map[string]any)
	if _, exists := public["decodeKey"]; exists {
		t.Fatal("decode key must not be emitted to the UI")
	}
	if public["source"] != "WeChat Channels" {
		t.Fatalf("unexpected source: %#v", public)
	}
	if len(s.resources) != 1 {
		t.Fatal("expected captured resource")
	}
	for _, captured := range s.resources {
		if captured.DecodeKey != "AQID" {
			t.Fatal("decode key was not retained for download")
		}
		if captured.Size != 42 {
			t.Fatalf("string file size was not parsed: %d", captured.Size)
		}
	}
}

func TestExtractWeChatMediaPreservesNumericDecodeKey(t *testing.T) {
	decoder := json.NewDecoder(bytes.NewBufferString(`{"data":{"object":{"objectDesc":{"description":"nested","mediaType":4,"media":[{"url":"https://finder.video.qq.com/file.mp4","urlToken":"token=2","fileSize":123456,"decodeKey":1844674407370955161}]}}}}`))
	decoder.UseNumber()
	var payload any
	if err := decoder.Decode(&payload); err != nil {
		t.Fatal(err)
	}
	items := extractWeChatMediaItems(payload, "")
	if len(items) == 0 {
		t.Fatal("expected nested WeChat media")
	}
	item := items[0]
	if item.DecodeKey != "1844674407370955161" {
		t.Fatalf("numeric decode key lost precision: %q", item.DecodeKey)
	}
	if item.Size != 123456 || item.Title != "nested" {
		t.Fatalf("unexpected metadata: %#v", item)
	}
	if got := joinURLToken(item.URL, item.URLToken); got != "https://finder.video.qq.com/file.mp4?token=2" {
		t.Fatalf("unexpected signed URL: %s", got)
	}
}

func TestSafeFilenameAlwaysAddsExpectedExtension(t *testing.T) {
	if got := safeFilename("part.1/demo", "mp4"); got != "part.1_demo.mp4" {
		t.Fatalf("unexpected filename: %s", got)
	}
}

func TestMITMAllowlistRejectsUnrelatedHosts(t *testing.T) {
	if !isAllowedHost("channels.weixin.qq.com") || !isAllowedHost("cdn.finder.video.qq.com") {
		t.Fatal("expected WeChat hosts to be allowed")
	}
	if isAllowedHost("example.com") || isAllowedHost("notqq.com") {
		t.Fatal("unrelated hosts must not be intercepted")
	}
}

func TestWeChatCoverURLNormalizesSchemeAndHost(t *testing.T) {
	item := map[string]any{
		"fullCoverUrl": "http://wxapp.tc.qq.com/251/cover.jpg?x=1",
		"url":          "https://finder.video.qq.com/251/v.mp4",
		"urlToken":     "tok",
	}
	if got := wechatCoverURL(item); got != "https://finder.video.qq.com/251/cover.jpg?x=1" {
		t.Fatalf("unexpected cover URL: %s", got)
	}
	if got := wechatCoverURL(map[string]any{}); got != "" {
		t.Fatalf("expected empty cover URL, got %s", got)
	}
}
