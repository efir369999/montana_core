package montanaxray

import (
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"crypto/sha256"
	"crypto/x509"
	"crypto/x509/pkix"
	"encoding/json"
	"encoding/pem"
	"fmt"
	"math/big"
	"strings"
	"testing"
	"time"

	"github.com/xtls/xray-core/core"
	"github.com/xtls/xray-core/infra/conf/serial"
)

// THE CONTROL FOR THE gRPC ROWS (29.09, T1: every grpc+tls row of the plan timed out from the phone, every reality row
// answered). Whether that is the network or our own config is decided here, on the loopback: a real VLESS server behind
// gRPC and TLS, and the client outbound written exactly as MontanaXrayConfig.outbound writes a `type=grpc&security=tls`
// link -- serverName, fingerprint, serviceName, multiMode. The wrong implementations this catches: a config the engine
// cannot dial (the answer would be an error here too), and a fingerprint (chrome, safari) the gRPC transport refuses.
// The control of the control: a wrong serviceName must fail, or the test measures nothing.

// The certificate, its key, and the hex sha256 of its DER -- the pin the engine takes in place of the removed allowInsecure.
func selfSignedLocalhost(t *testing.T) (string, string, string) {
	key, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	tmpl := x509.Certificate{
		SerialNumber: big.NewInt(1),
		Subject:      pkix.Name{CommonName: "localhost"},
		DNSNames:     []string{"localhost"},
		NotBefore:    time.Now().Add(-time.Hour),
		NotAfter:     time.Now().Add(24 * time.Hour),
		KeyUsage:     x509.KeyUsageDigitalSignature | x509.KeyUsageKeyEncipherment,
		ExtKeyUsage:  []x509.ExtKeyUsage{x509.ExtKeyUsageServerAuth},
	}
	der, err := x509.CreateCertificate(rand.Reader, &tmpl, &tmpl, &key.PublicKey, key)
	if err != nil {
		t.Fatal(err)
	}
	kb, err := x509.MarshalECPrivateKey(key)
	if err != nil {
		t.Fatal(err)
	}
	certPEM := string(pem.EncodeToMemory(&pem.Block{Type: "CERTIFICATE", Bytes: der}))
	keyPEM := string(pem.EncodeToMemory(&pem.Block{Type: "EC PRIVATE KEY", Bytes: kb}))
	sum := sha256.Sum256(der)
	return certPEM, keyPEM, fmt.Sprintf("%x", sum[:])
}

func pemLines(p string) string {
	b, _ := json.Marshal(strings.Split(strings.TrimSpace(p), "\n"))
	return string(b)
}

// A VLESS server behind gRPC and TLS on the loopback, the shape of the plan's grpc rows.
func grpcTLSServer(t *testing.T) (int, string) {
	port := freeTCPPort(t)
	cert, key, pin := selfSignedLocalhost(t)
	js := fmt.Sprintf(`{"log":{"loglevel":"none"},
	  "inbounds":[{"listen":"127.0.0.1","port":%d,"protocol":"vless","settings":{"clients":[{"id":"%s"}],"decryption":"none"},
	    "streamSettings":{"network":"grpc","security":"tls","tlsSettings":{"certificates":[{"certificate":%s,"key":%s}]},"grpcSettings":{"serviceName":"svc"}}}],
	  "outbounds":[{"protocol":"freedom","tag":"out"}]}`, port, testID, pemLines(cert), pemLines(key))
	cfg, err := serial.LoadJSONConfig(strings.NewReader(js))
	if err != nil {
		t.Fatal(err)
	}
	inst, err := core.New(cfg)
	if err != nil {
		t.Fatal(err)
	}
	if err := inst.Start(); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { inst.Close() })
	return port, pin
}

// The client outbound as the app writes it for a grpc+tls link: the TLS with serverName and fingerprint (the pinned hash
// here for the loopback certificate, as the app writes a link's pinSHA256), grpcSettings with the serviceName and, when the
// link says mode=multi, multiMode.
func grpcOutbound(port int, pin string, fingerprint string, multi bool, service string) string {
	tls := fmt.Sprintf(`{"serverName":"localhost","fingerprint":"%s","pinnedPeerCertSha256":"%s"}`, fingerprint, pin)
	grpc := fmt.Sprintf(`{"serviceName":"%s"}`, service)
	if multi {
		grpc = fmt.Sprintf(`{"serviceName":"%s","multiMode":true}`, service)
	}
	return fmt.Sprintf(`{"protocol":"vless","settings":{"vnext":[{"address":"127.0.0.1","port":%d,"users":[{"id":"%s","encryption":"none"}]}]},"streamSettings":{"network":"grpc","security":"tls","tlsSettings":%s,"grpcSettings":%s}}`, port, testID, tls, grpc)
}

func TestGRPCTLSRowsAsTheAppWritesThem(t *testing.T) {
	web := webOnLoopback(t)
	port, pin := grpcTLSServer(t)
	batch := "[" + strings.Join([]string{
		grpcOutbound(port, pin, "", false, "svc"),
		grpcOutbound(port, pin, "chrome", false, "svc"),
		grpcOutbound(port, pin, "safari", false, "svc"),
		grpcOutbound(port, pin, "chrome", true, "svc"),
		grpcOutbound(port, pin, "chrome", false, "wrong"),
	}, ",") + "]"
	out, err := MeasureMany(batch, web, 5, "", 6)
	if err != nil {
		t.Fatalf("the batch failed as a whole: %v", err)
	}
	var answers []probeAnswer
	if err := json.Unmarshal([]byte(out), &answers); err != nil {
		t.Fatal(err)
	}
	names := []string{"no fingerprint", "chrome", "safari", "chrome multiMode", "wrong serviceName (must fail)"}
	for i, a := range answers[:4] {
		if a.Err != "" || a.Ms < 0 {
			t.Errorf("grpc+tls %s: the app's own config failed on the loopback: %+v", names[i], a)
		}
	}
	if answers[4].Err == "" {
		t.Errorf("a wrong serviceName was measured as alive (%+v): the test measures nothing", answers[4])
	}
	t.Logf("grpc+tls on the loopback: %s", out)
}
