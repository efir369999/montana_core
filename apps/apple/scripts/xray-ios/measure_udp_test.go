package montanaxray

import (
	"fmt"
	"net"
	"net/http"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/xtls/xray-core/core"
	"github.com/xtls/xray-core/infra/conf/serial"
	_ "github.com/xtls/xray-core/proxy/blackhole"
	_ "github.com/xtls/xray-core/proxy/vless/inbound"
)

// The two wrong measurements this file is here to catch, named before they are caught:
//   1. one that says «no UDP» whatever the server does -- the server that carries UDP must answer;
//   2. one that asks our STUN past the server, directly -- through a server that refuses UDP our STUN
//      must hear nothing at all, and the verdict must be «no answer».
// Everything runs on the loopback: a VLESS server of the seller's shape (plain TCP, no flow), a STUN that
// answers a Binding Request with its own id, a web page that answers 204. No outside server, no secret.

const testID = "b831381d-6324-4d53-ad4f-8cda48b30811"

func freeTCPPort(t *testing.T) int {
	l, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	p := l.Addr().(*net.TCPAddr).Port
	l.Close()
	return p
}

func stunOnLoopback(t *testing.T) (string, func() int32) {
	pc, err := net.ListenPacket("udp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	var heard int32
	go func() {
		b := make([]byte, 1500)
		for {
			n, from, err := pc.ReadFrom(b)
			if err != nil {
				return
			}
			if n >= 20 && b[0] == 0x00 && b[1] == 0x01 {
				atomic.AddInt32(&heard, 1)
				resp := make([]byte, 20)
				resp[0], resp[1] = 0x01, 0x01
				copy(resp[4:20], b[4:20])
				pc.WriteTo(resp, from)
			}
		}
	}()
	t.Cleanup(func() { pc.Close() })
	return pc.LocalAddr().String(), func() int32 { return atomic.LoadInt32(&heard) }
}

func webOnLoopback(t *testing.T) string {
	l, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	srv := &http.Server{Handler: http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { w.WriteHeader(204) })}
	go srv.Serve(l)
	t.Cleanup(func() { srv.Close() })
	return "http://" + l.Addr().String() + "/generate_204"
}

// With blockUDP its router sends every UDP destination to a blackhole: the TCP-only entry of the fleet.
func vlessServer(t *testing.T, blockUDP bool) int {
	port := freeTCPPort(t)
	rules := ""
	if blockUDP {
		rules = `,"routing":{"rules":[{"type":"field","network":"udp","outboundTag":"block"}]}`
	}
	js := fmt.Sprintf(`{"log":{"loglevel":"none"},
	  "inbounds":[{"listen":"127.0.0.1","port":%d,"protocol":"vless","settings":{"clients":[{"id":"%s"}],"decryption":"none"}}],
	  "outbounds":[{"protocol":"freedom","tag":"out"},{"protocol":"blackhole","tag":"block"}]%s}`, port, testID, rules)
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
	return port
}

// The client as the app builds a server for a measurement (MontanaVPNDelay.probeConfig): the outbound and a
// direct one, nothing else.
func clientConfig(port int) string {
	return fmt.Sprintf(`{"log":{"loglevel":"none"},"outbounds":[
	  {"protocol":"vless","tag":"proxy","settings":{"vnext":[{"address":"127.0.0.1","port":%d,"users":[{"id":"%s","encryption":"none"}]}]},
	   "streamSettings":{"network":"tcp","security":"none"}},
	  {"protocol":"freedom","tag":"direct"}]}`, port, testID)
}

func TestMeasureUDPThroughAServerThatCarriesIt(t *testing.T) {
	stun, heard := stunOnLoopback(t)
	port := vlessServer(t, false)
	ms, err := MeasureUDP(clientConfig(port), stun, "")
	if err != nil {
		t.Fatalf("a server that carries UDP was measured as refusing it: %v", err)
	}
	if ms < 0 || ms > 3000 {
		t.Fatalf("milliseconds outside the window: %d", ms)
	}
	if heard() == 0 {
		t.Fatal("the answer did not come from our STUN")
	}
	t.Logf("UDP through the server: %d ms, our STUN heard %d request(s)", ms, heard())
}

func TestMeasureUDPThroughATCPOnlyServer(t *testing.T) {
	stun, heard := stunOnLoopback(t)
	port := vlessServer(t, true)
	// The control first: TCP passes this very server. The entry is alive; only UDP is refused.
	ms, err := MeasureDelay(clientConfig(port), webOnLoopback(t), "")
	if err != nil {
		t.Fatalf("TCP did not pass the server: %v", err)
	}
	t.Logf("TCP through the server: %d ms", ms)
	start := time.Now()
	u, err := MeasureUDP(clientConfig(port), stun, "")
	if err == nil {
		t.Fatalf("a TCP-only server was measured as carrying UDP (%d ms)", u)
	}
	if n := heard(); n != 0 {
		t.Fatalf("our STUN heard %d request(s) through a server that refuses UDP -- the measurement went past it", n)
	}
	if took := time.Since(start); took > 4*time.Second {
		t.Fatalf("the verdict took %v, past the window", took)
	}
	t.Logf("TCP-only server: %v (in %v)", err, time.Since(start).Round(time.Millisecond))
}
