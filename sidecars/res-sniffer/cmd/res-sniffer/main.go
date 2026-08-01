// Cobalt Resource Sniffer is a local-only, opt-in capture proxy.
// It intentionally MITMs a small allowlist instead of all HTTPS traffic.
package main

import (
	"bufio"
	"bytes"
	"compress/gzip"
	"crypto/rand"
	"crypto/rsa"
	"crypto/sha256"
	"crypto/tls"
	"crypto/x509"
	"crypto/x509/pkix"
	"encoding/hex"
	"encoding/json"
	"encoding/pem"
	"flag"
	"fmt"
	"io"
	"math/big"
	"net"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/elazarl/goproxy"
)

const version = "0.1.0"

type command struct {
	ID            string `json:"id"`
	Command       string `json:"command"`
	Port          int    `json:"port"`
	UpstreamProxy string `json:"upstreamProxy"`
	ResourceID    string `json:"resourceId"`
}
type message struct {
	ID    string `json:"id,omitempty"`
	Type  string `json:"type"`
	OK    bool   `json:"ok,omitempty"`
	Error string `json:"error,omitempty"`
	Data  any    `json:"data,omitempty"`
}
type resource struct {
	ID         string      `json:"id"`
	Title      string      `json:"title"`
	Source     string      `json:"source"`
	Kind       string      `json:"kind"`
	MimeType   string      `json:"mimeType"`
	Size       int64       `json:"size"`
	Extension  string      `json:"extension"`
	CapturedAt string      `json:"capturedAt"`
	URL        string      `json:"-"`
	Headers    http.Header `json:"-"`
	DecodeKey  string      `json:"-"`
}

var allowedDomains = []string{
	"channels.weixin.qq.com", "res.wx.qq.com", "finder.video.qq.com", "wx.qlogo.cn", "mp.weixin.qq.com", "wx.qq.com",
	"douyin.com", "douyinvod.com", "iesdouyin.com", "byteoversea.com",
	"kuaishou.com", "kwai.com",
	"xiaohongshu.com", "xhslink.com",
	"bilibili.com", "bilivideo.com", "hdslb.com",
	"kugou.com", "qqmusic.qq.com", "music.qq.com", "y.qq.com",
	"iqiyi.com", "youku.com", "v.qq.com",
}

type server struct {
	mu              sync.RWMutex
	resources       map[string]resource
	out             chan<- message
	httpServer      *http.Server
	dataDir         string
	wechatHooks     int
	wechatCallbacks int
}

func main() {
	dataDir := flag.String("data-dir", "", "directory for local state")
	flag.Parse()
	if *dataDir == "" {
		fmt.Fprintln(os.Stderr, "--data-dir is required")
		os.Exit(2)
	}
	if err := os.MkdirAll(*dataDir, 0700); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(2)
	}

	out := make(chan message, 32)
	go func() {
		encoder := json.NewEncoder(os.Stdout)
		for item := range out {
			_ = encoder.Encode(item)
		}
	}()
	out <- message{Type: "ready", OK: true, Data: map[string]any{"version": version, "allowedDomains": allowedDomains}}

	s := &server{resources: make(map[string]resource), out: out, dataDir: *dataDir}
	scanner := bufio.NewScanner(os.Stdin)
	for scanner.Scan() {
		s.handle(scanner.Bytes())
	}
}

func (s *server) handle(line []byte) {
	var c command
	if err := json.Unmarshal(line, &c); err != nil {
		s.out <- message{Type: "error", Error: "Invalid sidecar command"}
		return
	}
	switch c.Command {
	case "status":
		s.mu.RLock()
		resourceCount, hookCount, callbackCount := len(s.resources), s.wechatHooks, s.wechatCallbacks
		s.mu.RUnlock()
		s.out <- message{ID: c.ID, Type: "response", OK: true, Data: map[string]any{"running": s.httpServer != nil, "resourceCount": resourceCount, "wechatHooks": hookCount, "wechatCallbacks": callbackCount, "allowedDomains": allowedDomains}}
	case "clear":
		s.mu.Lock()
		s.resources = make(map[string]resource)
		s.mu.Unlock()
		s.out <- message{ID: c.ID, Type: "response", OK: true}
	case "resolve_download":
		s.mu.RLock()
		r, ok := s.resources[c.ResourceID]
		s.mu.RUnlock()
		if !ok {
			s.out <- message{ID: c.ID, Type: "response", Error: "Captured resource is no longer available"}
			return
		}
		// This descriptor travels only through the stdin/stdout control pipe to Rust.
		// It is never emitted as a browser event, persisted, or returned to the UI.
		s.out <- message{ID: c.ID, Type: "response", OK: true, Data: map[string]any{"url": r.URL, "headers": r.Headers, "filename": safeFilename(r.Title, r.Extension), "kind": r.Kind, "decodeKey": r.DecodeKey}}
	case "start":
		if err := s.start(c.Port, c.UpstreamProxy); err != nil {
			s.out <- message{ID: c.ID, Type: "response", Error: err.Error()}
			return
		}
		s.out <- message{ID: c.ID, Type: "response", OK: true, Data: map[string]any{"port": c.Port}}
	case "stop":
		if s.httpServer != nil {
			_ = s.httpServer.Close()
			s.httpServer = nil
		}
		s.out <- message{ID: c.ID, Type: "response", OK: true}
	default:
		s.out <- message{ID: c.ID, Type: "response", Error: "Unknown sidecar command"}
	}
}

func (s *server) start(port int, upstream string) error {
	if s.httpServer != nil {
		return nil
	}
	if port == 0 {
		port = 8899
	}
	certificate, certPath, fingerprint, err := localCA(s.dataDir)
	if err != nil {
		return fmt.Errorf("unable to create local capture certificate: %w", err)
	}
	// goproxy uses this CA to mint per-host certificates for our allowlist only.
	// It is generated per installation, never committed or shared between users.
	goproxy.GoproxyCa = *certificate
	proxy := goproxy.NewProxyHttpServer()
	proxy.Verbose = false
	// Never use goproxy's default "MITM every CONNECT" behavior.
	proxy.OnRequest().HandleConnectFunc(func(host string, ctx *goproxy.ProxyCtx) (*goproxy.ConnectAction, string) {
		cleanHost := strings.Split(strings.ToLower(host), ":")[0]
		allowed := isAllowedHost(cleanHost)
		if allowed {
			return goproxy.MitmConnect, host
		}
		return goproxy.OkConnect, host
	})
	proxy.OnRequest().DoFunc(func(req *http.Request, _ *goproxy.ProxyCtx) (*http.Request, *http.Response) {
		if req.URL.Hostname() == "channels.weixin.qq.com" && req.URL.Path == "/__cobalt/hook.js" {
			return req, staticResponse(req, "application/javascript; charset=utf-8", []byte(wechatHookScript))
		}
		if req.URL.Hostname() == "channels.weixin.qq.com" && req.URL.Path == "/__cobalt/wechat" {
			// The request body belongs to the proxy transaction. Consume it before
			// returning instead of racing the transport from a goroutine.
			s.captureWeChatCallback(req)
			return req, emptyResponse(req)
		}
		// Restrict WeChat asset encoding to gzip/identity so the proxy can safely inspect/rewrite JS hooks.
		if strings.HasSuffix(req.URL.Hostname(), "res.wx.qq.com") || strings.HasSuffix(req.URL.Hostname(), "channels.weixin.qq.com") {
			req.Header.Set("Accept-Encoding", "gzip, identity")
		}
		return req, nil
	})
	proxy.OnResponse().DoFunc(func(resp *http.Response, ctx *goproxy.ProxyCtx) *http.Response {
		if resp == nil || ctx.Req == nil || !isAllowedHost(ctx.Req.URL.Hostname()) {
			return resp
		}
		if !isMedia(resp.Header.Get("Content-Type"), ctx.Req.URL.Path) {
			return s.injectWeChatHook(resp, ctx.Req)
		}
		// Finder video responses are encrypted. The metadata callback below adds
		// the usable item together with its decode key, so exposing this raw stream
		// would create a duplicate download that can never play.
		if ctx.Req.URL.Hostname() == "finder.video.qq.com" {
			return resp
		}
		s.capture(ctx.Req, resp)
		return resp
	})
	if upstream != "" {
		parsed, err := url.Parse(upstream)
		if err != nil {
			return fmt.Errorf("invalid upstream proxy")
		}
		proxy.Tr = &http.Transport{Proxy: http.ProxyURL(parsed)}
	}
	listener, err := net.Listen("tcp", fmt.Sprintf("127.0.0.1:%d", port))
	if err != nil {
		return err
	}
	s.httpServer = &http.Server{Handler: proxy, ReadHeaderTimeout: 10 * time.Second}
	go func() { _ = s.httpServer.Serve(listener) }()
	s.out <- message{Type: "status", OK: true, Data: map[string]any{"running": true, "port": port, "certificatePath": certPath, "certificateFingerprint": fingerprint}}
	return nil
}

func (s *server) capture(req *http.Request, resp *http.Response) {
	id := randomID()
	mime := strings.ToLower(strings.Split(resp.Header.Get("Content-Type"), ";")[0])
	kind := "video"
	if strings.HasPrefix(mime, "audio/") {
		kind = "audio"
	}
	if strings.Contains(mime, "mpegurl") || strings.Contains(mime, "dash+xml") {
		kind = "playlist"
	}
	ext := extensionFor(mime, req.URL.Path)
	r := resource{ID: id, Title: filepath.Base(req.URL.Path), Source: req.URL.Hostname(), Kind: kind, MimeType: mime, Size: resp.ContentLength, Extension: ext, CapturedAt: time.Now().UTC().Format(time.RFC3339), URL: req.URL.String(), Headers: req.Header.Clone()}
	if r.Title == "." || r.Title == "/" || r.Title == "" {
		r.Title = r.Source + " media"
	}
	s.mu.Lock()
	for _, old := range s.resources {
		if old.URL == r.URL {
			s.mu.Unlock()
			return
		}
	}
	s.resources[id] = r
	s.mu.Unlock()
	// Only send UI-safe metadata. Signed URLs and headers stay inside the sidecar.
	s.out <- message{Type: "resource", OK: true, Data: publicResource(r)}
}

const wechatHookTag = `<script src="/__cobalt/hook.js"></script>`

const wechatHookScript = `(function(){
if(window.__cobalt_hooked)return;
window.__cobalt_hooked=true;
function report(obj){
  if(!obj||typeof obj!=="object")return;
  var body;
  try{body=JSON.stringify(obj);}catch(_){return;}
  try{
    if(navigator.sendBeacon&&navigator.sendBeacon("/__cobalt/wechat",new Blob([body],{type:"application/json"})))return;
  }catch(_){}
  fetch("/__cobalt/wechat",{method:"POST",headers:{"Content-Type":"application/json"},body:body,keepalive:true}).catch(function(){});
}
window.__cobalt_report=report;
var originalFetch=window.fetch;
if(originalFetch){window.fetch=function(){return originalFetch.apply(this,arguments).then(function(response){try{response.clone().json().then(report).catch(function(){});}catch(_){}return response;});};}
var originalOpen=XMLHttpRequest.prototype.open,originalSend=XMLHttpRequest.prototype.send;
XMLHttpRequest.prototype.open=function(method,url){this.__cobalt_url=url;return originalOpen.apply(this,arguments);};
XMLHttpRequest.prototype.send=function(){this.addEventListener("load",function(){try{report(JSON.parse(this.responseText));}catch(_){}});return originalSend.apply(this,arguments);};
})();`

type wechatBridgePatch struct {
	pattern     *regexp.Regexp
	replacement string
}

var wechatBridgePatches = []wechatBridgePatch{
	{regexp.MustCompile(`async finderInit\(\)\{(.*?)\}async`), `async finderInit(){var __cobalt_result=await(async()=>{$1})();try{window.__cobalt_report(__cobalt_result&&__cobalt_result.data)}catch(_){}return __cobalt_result}async`},
	{regexp.MustCompile(`async finderPcFlow\((\w+)\)\{(.*?)\}async`), `async finderPcFlow($1){var __cobalt_result=await(async()=>{$2})();try{window.__cobalt_report(__cobalt_result&&__cobalt_result.data)}catch(_){}return __cobalt_result}async`},
	{regexp.MustCompile(`async finderGetRecommend\((\w+)\)\{(.*?)\}async`), `async finderGetRecommend($1){var __cobalt_result=await(async()=>{$2})();try{window.__cobalt_report(__cobalt_result&&__cobalt_result.data)}catch(_){}return __cobalt_result}async`},
	{regexp.MustCompile(`async finderGetCommentDetail\((\w+)\)\{(.*?)\}async`), `async finderGetCommentDetail($1){var __cobalt_result=await(async()=>{$2})();try{window.__cobalt_report(__cobalt_result&&__cobalt_result.data)}catch(_){}return __cobalt_result}async`},
	{regexp.MustCompile(`async finderUserPage\((\w+)\)\{(.*?)\}async`), `async finderUserPage($1){var __cobalt_result=await(async()=>{$2})();try{window.__cobalt_report(__cobalt_result&&__cobalt_result.data)}catch(_){}return __cobalt_result}async`},
	{regexp.MustCompile(`async finderPCSearch\((\w+)\)\{(.*?)\}async`), `async finderPCSearch($1){var __cobalt_result=await(async()=>{$2})();try{window.__cobalt_report(__cobalt_result&&__cobalt_result.data)}catch(_){}return __cobalt_result}async`},
	{regexp.MustCompile(`async finderSearch\((\w+)\)\{(.*?)\}async`), `async finderSearch($1){var __cobalt_result=await(async()=>{$2})();try{window.__cobalt_report(__cobalt_result&&__cobalt_result.data)}catch(_){}return __cobalt_result}async`},
}

func (s *server) injectWeChatHook(resp *http.Response, req *http.Request) *http.Response {
	if !strings.HasSuffix(req.URL.Hostname(), "qq.com") {
		return resp
	}
	ct := strings.ToLower(resp.Header.Get("Content-Type"))
	isHTML := strings.Contains(ct, "text/html")
	isBridgeScript := strings.Contains(ct, "javascript") && strings.Contains(req.URL.Path, "virtual_svg-icons-register.publish")
	if !isHTML && !isBridgeScript {
		return resp
	}
	body, encoding, ok := readResponseBody(resp)
	if !ok {
		return resp
	}
	if isHTML {
		injected := bytes.Replace(body, []byte("<head>"), []byte("<head>"+wechatHookTag), 1)
		if bytes.Equal(body, injected) {
			injected = append([]byte(wechatHookTag), body...)
		}
		s.mu.Lock()
		s.wechatHooks++
		hooks := s.wechatHooks
		s.mu.Unlock()
		s.out <- message{Type: "status", OK: true, Data: map[string]any{"wechatHooks": hooks, "message": "WeChat Channels HTML script injected"}}
		return writeResponseBody(resp, injected, encoding)
	}
	injected := body
	for _, patch := range wechatBridgePatches {
		injected = patch.pattern.ReplaceAll(injected, []byte(patch.replacement))
	}
	if bytes.Equal(body, injected) {
		return resp
	}
	s.mu.Lock()
	s.wechatHooks++
	hooks := s.wechatHooks
	s.mu.Unlock()
	s.out <- message{Type: "status", OK: true, Data: map[string]any{"wechatHooks": hooks, "message": "WeChat Channels JS hook injected"}}
	return writeResponseBody(resp, injected, encoding)
}

func (s *server) captureWeChatCallback(req *http.Request) {
	body, err := io.ReadAll(io.LimitReader(req.Body, 2<<20))
	if err != nil {
		return
	}
	var raw any
	decoder := json.NewDecoder(bytes.NewReader(body))
	decoder.UseNumber()
	if decoder.Decode(&raw) != nil {
		return
	}
	items := extractWeChatMediaItems(raw, "")
	for _, item := range items {
		if item.URL == "" {
			continue
		}
		title := item.Title
		if title == "" {
			title = "WeChat Channels video"
		}
		kind, mime, extension := "video", "video/mp4", "mp4"
		if item.MediaType == 9 {
			kind, mime, extension = "image", "image/png", "png"
		}
		r := resource{
			ID:         randomID(),
			Title:      title,
			Source:     "WeChat Channels",
			Kind:       kind,
			MimeType:   mime,
			Size:       item.Size,
			Extension:  extension,
			CapturedAt: time.Now().UTC().Format(time.RFC3339),
			URL:        joinURLToken(item.URL, item.URLToken),
			Headers:    http.Header{"Referer": []string{"https://channels.weixin.qq.com/"}, "Origin": []string{"https://channels.weixin.qq.com/"}},
			DecodeKey:  item.DecodeKey,
		}
		s.mu.Lock()
		already := false
		for _, old := range s.resources {
			if old.URL == r.URL {
				already = true
				break
			}
		}
		if !already {
			s.resources[r.ID] = r
			s.wechatCallbacks++
		}
		s.mu.Unlock()
		if !already {
			s.out <- message{Type: "resource", OK: true, Data: publicResource(r)}
		}
	}
}

type wechatCapturedItem struct {
	URL       string
	URLToken  string
	DecodeKey string
	Title     string
	Size      int64
	MediaType int
}

func extractWeChatMediaItems(val any, parentTitle string) []wechatCapturedItem {
	var results []wechatCapturedItem
	switch v := val.(type) {
	case map[string]any:
		currentTitle := parentTitle
		if desc, ok := v["description"].(string); ok && desc != "" {
			currentTitle = desc
		}
		if mediaList, ok := v["media"].([]any); ok {
			for _, m := range mediaList {
				results = append(results, extractWeChatMediaItems(m, currentTitle)...)
			}
		}
		if descObj, ok := v["objectDesc"].(map[string]any); ok {
			results = append(results, extractWeChatMediaItems(descObj, currentTitle)...)
		}
		urlStr := scalarString(v["url"])
		urlToken := scalarString(v["urlToken"])
		decodeKey := scalarString(v["decodeKey"])
		if urlStr != "" && (urlToken != "" || decodeKey != "" || strings.Contains(urlStr, "finder.video.qq.com")) {
			size := scalarInt64(v["fileSize"])
			mType := int(scalarInt64(v["mediaType"]))
			results = append(results, wechatCapturedItem{
				URL:       urlStr,
				URLToken:  urlToken,
				DecodeKey: decodeKey,
				Title:     currentTitle,
				Size:      size,
				MediaType: mType,
			})
		}
		for _, child := range v {
			results = append(results, extractWeChatMediaItems(child, currentTitle)...)
		}
	case []any:
		for _, item := range v {
			results = append(results, extractWeChatMediaItems(item, parentTitle)...)
		}
	}
	return results
}

func scalarString(value any) string {
	switch v := value.(type) {
	case string:
		return v
	case json.Number:
		return v.String()
	case float64:
		return strconv.FormatFloat(v, 'f', -1, 64)
	default:
		return ""
	}
}

func scalarInt64(value any) int64 {
	text := scalarString(value)
	parsed, _ := strconv.ParseInt(text, 10, 64)
	return parsed
}

func joinURLToken(mediaURL, token string) string {
	if token == "" || strings.HasSuffix(mediaURL, token) {
		return mediaURL
	}
	if strings.HasPrefix(token, "?") || strings.HasPrefix(token, "&") {
		return mediaURL + token
	}
	if strings.Contains(mediaURL, "?") {
		return mediaURL + "&" + token
	}
	return mediaURL + "?" + token
}

func emptyResponse(req *http.Request) *http.Response {
	return staticResponse(req, "text/plain; charset=utf-8", []byte("ok"))
}
func staticResponse(req *http.Request, contentType string, body []byte) *http.Response {
	return &http.Response{Status: "200 OK", StatusCode: http.StatusOK, Header: http.Header{"Content-Type": []string{contentType}, "Content-Length": []string{strconv.Itoa(len(body))}}, Body: io.NopCloser(bytes.NewReader(body)), ContentLength: int64(len(body)), Request: req}
}
func readResponseBody(resp *http.Response) ([]byte, string, bool) {
	raw, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, "", false
	}
	_ = resp.Body.Close()
	encoding := strings.ToLower(resp.Header.Get("Content-Encoding"))
	if encoding == "" || encoding == "identity" {
		return raw, encoding, true
	}
	if encoding == "gzip" {
		reader, err := gzip.NewReader(bytes.NewReader(raw))
		if err != nil {
			return nil, "", false
		}
		decoded, err := io.ReadAll(reader)
		_ = reader.Close()
		return decoded, encoding, err == nil
	}
	return nil, "", false
}
func writeResponseBody(resp *http.Response, body []byte, encoding string) *http.Response {
	var output []byte
	if encoding == "gzip" {
		var buffer bytes.Buffer
		writer := gzip.NewWriter(&buffer)
		if _, err := writer.Write(body); err != nil {
			return resp
		}
		_ = writer.Close()
		output = buffer.Bytes()
	} else {
		output = body
	}
	resp.Body = io.NopCloser(bytes.NewReader(output))
	resp.ContentLength = int64(len(output))
	resp.Header.Set("Content-Length", fmt.Sprint(len(output)))
	return resp
}

func publicResource(r resource) map[string]any {
	return map[string]any{"id": r.ID, "title": r.Title, "source": r.Source, "kind": r.Kind, "mimeType": r.MimeType, "size": r.Size, "extension": r.Extension, "capturedAt": r.CapturedAt}
}
func isAllowedHost(host string) bool {
	host = strings.ToLower(host)
	for _, domain := range allowedDomains {
		domain = strings.ToLower(domain)
		if host == domain || strings.HasSuffix(host, "."+domain) {
			return true
		}
	}
	return false
}
func isMedia(contentType string, path string) bool {
	v := strings.ToLower(contentType)
	p := strings.ToLower(path)
	if strings.HasPrefix(v, "video/") || strings.HasPrefix(v, "audio/") || strings.Contains(v, "mpegurl") || strings.Contains(v, "dash+xml") || strings.Contains(v, "octet-stream") {
		return true
	}
	ext := filepath.Ext(p)
	return ext == ".mp4" || ext == ".m3u8" || ext == ".flv" || ext == ".ts" || ext == ".m4s" || ext == ".mp3" || ext == ".aac" || ext == ".m4a"
}
func extensionFor(mime, path string) string {
	if ext := filepath.Ext(path); len(ext) > 1 && len(ext) < 8 {
		return strings.TrimPrefix(ext, ".")
	}
	if strings.HasPrefix(mime, "audio/") {
		return "m4a"
	}
	if strings.Contains(mime, "mpegurl") {
		return "m3u8"
	}
	return "mp4"
}
func safeFilename(title, extension string) string {
	title = strings.TrimSpace(title)
	if title == "" {
		title = "captured-media"
	}
	title = strings.Map(func(r rune) rune {
		if r < 32 || strings.ContainsRune(`/\\:*?"<>|`, r) {
			return '_'
		}
		return r
	}, title)
	title = strings.Trim(title, " .")
	if title == "" {
		title = "captured-media"
	}
	extension = strings.TrimPrefix(strings.ToLower(extension), ".")
	if extension != "" && !strings.HasSuffix(strings.ToLower(title), "."+extension) {
		return title + "." + extension
	}
	return title
}
func randomID() string {
	b := make([]byte, 12)
	if _, err := rand.Read(b); err != nil {
		return fmt.Sprintf("capture-%d", time.Now().UnixNano())
	}
	return hex.EncodeToString(b)
}

var _ io.Reader

func localCA(dataDir string) (*tls.Certificate, string, string, error) {
	certPath, keyPath := filepath.Join(dataDir, "cobalt-capture-ca.pem"), filepath.Join(dataDir, "cobalt-capture-ca-key.pem")
	if cert, err := tls.LoadX509KeyPair(certPath, keyPath); err == nil {
		fingerprint, fpErr := certificateFingerprint(cert.Certificate[0])
		return &cert, certPath, fingerprint, fpErr
	}
	key, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		return nil, "", "", err
	}
	serialLimit := new(big.Int).Lsh(big.NewInt(1), 128)
	serial, err := rand.Int(rand.Reader, serialLimit)
	if err != nil {
		return nil, "", "", err
	}
	template := &x509.Certificate{SerialNumber: serial, Subject: pkix.Name{CommonName: "Cobalt Local Resource Capture CA"}, NotBefore: time.Now().Add(-time.Hour), NotAfter: time.Now().AddDate(5, 0, 0), IsCA: true, BasicConstraintsValid: true, KeyUsage: x509.KeyUsageCertSign | x509.KeyUsageDigitalSignature | x509.KeyUsageCRLSign}
	der, err := x509.CreateCertificate(rand.Reader, template, template, &key.PublicKey, key)
	if err != nil {
		return nil, "", "", err
	}
	certOut, err := os.OpenFile(certPath, os.O_WRONLY|os.O_CREATE|os.O_TRUNC, 0600)
	if err != nil {
		return nil, "", "", err
	}
	_ = pem.Encode(certOut, &pem.Block{Type: "CERTIFICATE", Bytes: der})
	_ = certOut.Close()
	keyOut, err := os.OpenFile(keyPath, os.O_WRONLY|os.O_CREATE|os.O_TRUNC, 0600)
	if err != nil {
		return nil, "", "", err
	}
	_ = pem.Encode(keyOut, &pem.Block{Type: "RSA PRIVATE KEY", Bytes: x509.MarshalPKCS1PrivateKey(key)})
	_ = keyOut.Close()
	cert, err := tls.LoadX509KeyPair(certPath, keyPath)
	if err != nil {
		return nil, "", "", err
	}
	fingerprint, err := certificateFingerprint(cert.Certificate[0])
	return &cert, certPath, fingerprint, err
}
func certificateFingerprint(der []byte) (string, error) {
	if _, err := x509.ParseCertificate(der); err != nil {
		return "", err
	}
	sum := sha256.Sum256(der)
	return strings.ToUpper(hex.EncodeToString(sum[:])), nil
}
