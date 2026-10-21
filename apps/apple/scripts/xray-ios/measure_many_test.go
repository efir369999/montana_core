package montanaxray

import (
	"encoding/json"
	"fmt"
	"strings"
	"testing"
	"time"
)

// The wrong batches this file is here to catch, named before they are caught:
//   1. a batch that answers every row from ONE outbound -- three servers, one of them dead, must give two numbers and one
//      error, each on its own row (the forced tag really steers the dial);
//   2. a batch that dies with its first refused outbound -- a row the loader refuses answers its own error and the others
//      are still measured;
//   3. a batch that measures in turn -- three servers at width three take about one server's time, not three;
//   4. a UDP batch that says «carries» for a TCP-only server, or «refuses» for one that carries.
// Everything runs on the loopback: VLESS servers of the seller's shape, a STUN, a web page that answers 204.

func outboundJSON(port int) string {
	return fmt.Sprintf(`{"protocol":"vless","settings":{"vnext":[{"address":"127.0.0.1","port":%d,"users":[{"id":"%s","encryption":"none"}]}]},"streamSettings":{"network":"tcp","security":"none"}}`, port, testID)
}

func slowVlessServer(t *testing.T) int {
	// A server that accepts the handshake and answers nothing: the batch must name it timeout and not wait for it.
	return freeTCPPort(t)
}

func TestMeasureManyGivesEachRowItsOwnAnswer(t *testing.T) {
	web := webOnLoopback(t)
	a := vlessServer(t, false)
	b := vlessServer(t, true)
	dead := slowVlessServer(t)
	batch := "[" + strings.Join([]string{outboundJSON(a), outboundJSON(dead), outboundJSON(b), `{"protocol":"vless","settings":{}}`}, ",") + "]"
	start := time.Now()
	out, err := MeasureMany(batch, web, 6, "", 3)
	if err != nil {
		t.Fatalf("the batch failed as a whole: %v", err)
	}
	took := time.Since(start)
	var answers []probeAnswer
	if err := json.Unmarshal([]byte(out), &answers); err != nil {
		t.Fatalf("the answer is not a JSON array: %v -- %s", err, out)
	}
	if len(answers) != 4 {
		t.Fatalf("four rows asked, %d answered: %s", len(answers), out)
	}
	if answers[0].Err != "" || answers[0].Ms < 0 {
		t.Fatalf("the first live server was not measured: %+v", answers[0])
	}
	if answers[2].Err != "" || answers[2].Ms < 0 {
		t.Fatalf("the second live server was not measured: %+v", answers[2])
	}
	if answers[1].Err == "" {
		t.Fatalf("the dead server answered a number: %+v", answers[1])
	}
	if answers[3].Err == "" {
		t.Fatalf("the refused outbound answered a number: %+v", answers[3])
	}
	if took > 5*time.Second {
		t.Fatalf("the batch took %v: it waited for the dead one past the deadline, or measured in turn", took)
	}
	t.Logf("batch: %s in %v", out, took.Round(time.Millisecond))
}

func TestMeasureManyRunsWide(t *testing.T) {
	web := webOnLoopback(t)
	// Three dead servers, each worth the whole deadline: in turn they cost three deadlines, at width three one.
	batch := "[" + strings.Join([]string{outboundJSON(slowVlessServer(t)), outboundJSON(slowVlessServer(t)), outboundJSON(slowVlessServer(t))}, ",") + "]"
	start := time.Now()
	if _, err := MeasureMany(batch, web, 3, "", 2); err != nil {
		t.Fatal(err)
	}
	if took := time.Since(start); took > 4*time.Second {
		t.Fatalf("three at width three took %v: they were measured in turn", took)
	}
}

func TestMeasureUDPManyTellsCarriersFromTCPOnly(t *testing.T) {
	stun, heard := stunOnLoopback(t)
	carries := vlessServer(t, false)
	tcpOnly := vlessServer(t, true)
	batch := "[" + outboundJSON(carries) + "," + outboundJSON(tcpOnly) + "]"
	out, err := MeasureUDPMany(batch, stun, 2, "", 4)
	if err != nil {
		t.Fatal(err)
	}
	var answers []probeAnswer
	if err := json.Unmarshal([]byte(out), &answers); err != nil {
		t.Fatal(err)
	}
	if len(answers) != 2 || answers[0].Err != "" || answers[0].Ms < 0 {
		t.Fatalf("the server that carries UDP was not measured: %s", out)
	}
	if answers[1].Err == "" {
		t.Fatalf("the TCP-only server was measured as carrying UDP: %s", out)
	}
	if heard() != 1 {
		t.Fatalf("our STUN heard %d request(s); exactly one road carries them", heard())
	}
	t.Logf("udp batch: %s", out)
}
