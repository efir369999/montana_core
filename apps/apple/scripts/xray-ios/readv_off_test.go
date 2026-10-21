package montanaxray

import (
	"net"
	"testing"

	"github.com/xtls/xray-core/common/buf"
)

// Proof that the linkname reached the core's own variable rather than declaring a second one: with the
// batch strategy on, the core hands back a readv reader for a socket; with it off, a plain one.
func TestReadvIsOff(t *testing.T) {
	l, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer l.Close()
	go func() {
		c, err := l.Accept()
		if err == nil {
			defer c.Close()
		}
	}()
	c, err := net.Dial("tcp", l.Addr().String())
	if err != nil {
		t.Fatal(err)
	}
	defer c.Close()

	if got := typeName(buf.NewReader(c)); got == "*buf.ReadVReader" {
		t.Fatalf("core still reads in batches: NewReader gave %s", got)
	} else {
		t.Logf("batched reads off: NewReader gave %s", got)
	}

	// Flipping it back must bring the batched reader back. Without this the test could not fail, and a
	// test that cannot fail proves nothing about which variable the linkname reached.
	xrayUseReadv = true
	defer func() { xrayUseReadv = false }()
	if got := typeName(buf.NewReader(c)); got != "*buf.ReadVReader" {
		t.Fatalf("linkname did not reach the core's variable: NewReader gave %s", got)
	}
	t.Log("control: turning it back on restores the batched reader")
}

func typeName(v any) string {
	switch v.(type) {
	case *buf.ReadVReader:
		return "*buf.ReadVReader"
	}
	return "plain reader"
}
