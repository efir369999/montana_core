#!/usr/bin/env python3
"""Exhaustive check of Lemma 5 (c) of whitepaper.md: crossing pipe heals never leave two
correspondents unable to read each other, under every pattern of dropped, late and reordered
letters.

The model follows Network, the paragraphs on healing a pipe (Network:304-319 and :329-330 at
390441f). Two sides X and Y hold one old walk "w". The adversary delivers any letter in flight,
in any order, at any time, or drops it. Windows advance, and each side writes when it likes,
within a budget. A letter carries one heal part, the answer it owes before an offer of its own.
Encapsulation keys are drawn in an order that every run permutes, so every byte
order of the keys is covered. The adversary alters and injects nothing, as Lemma 5 assumes.

Checked in every reachable state:
  SAFE  the walk each side sends under is held by the other side.
A side that sends under a walk the other side has erased, or never had, can no longer be read.
That is the divergence Lemma 5 (c) rules out.
  ONE   no side holds two healed walks from one old walk (Network:330 calls that a defect).
ONE is reported, not required: the checker shows under which readings the defect is reachable.

Two mutants show that the checker finds a fault when one exists. Each must come out UNSAFE:
  no-rho  the offering side opens an answer without checking that it names its pending offer;
  early   the answering side moves to the healed walk on any letter of the other side.

The text leaves two points open, and the checker runs every reading of each:
  OFFER   strict: a side offers only while it holds one walk and nothing else, so not while it
          has answered a heal it has not yet switched to. loose: an answered heal does not stop it.
  STALE   what a side does when it opens an offer of an earlier window while it has an offer of
          its own pending. compare: the byte-order rule applies anyway. answer: it answers and
          keeps its own offer. ignore: it drops the offer.
  LETGO   off: the text as written. ios: the rule of the iOS client (MTPipeKey.swift:802-807 at
          d20b2d66), which Network does not state. A side that answered a heal and has not yet
          moved to it lets that heal go when the other side offers again, on the ground that the
          other side offers only while it holds one walk and so never opened the answer. A side
          that is offering, and still holds the old walk, ignores an offer. later: the same, but
          only an offer of a later window than the one answered lets the heal go; an offer of an
          earlier window is dropped. A late letter can carry an offer older than the one answered,
          so the ground of ios does not hold for it.

Run: python3 -B check_lemma5.py. Exit code 0: every reading with LETGO off or later is SAFE;
ONE holds for the proposed rule (LETGO later, OFFER strict, STALE compare, the reading of the iOS
client with its let-go corrected); both mutants are found. LETGO ios is expected UNSAFE, and its
shortest run is printed as the finding.
"""
import itertools
import sys
from collections import deque, namedtuple

WMAX, BUDGET, DRAWS = 2, 2, 4
# pend: (key, window, base) of its own offer. fold: the healed walk it folded as offering side.
# answered: healed walks it folded as answering side. owe: offers it has decided to answer.
Side = namedtuple("Side", "held send pend fold answered answered_keys owe lastw sent awin")
Letter = namedtuple("Letter", "frm walk offer answer")   # offer: (key, window); answer: key


def healed(base, key):
    return ("h", base, key)


def start():
    s = Side(frozenset({"w"}), "w", None, None, frozenset(), frozenset(), (), 0, 0, 0)
    return (1, (s, s), (), 0)


def can_offer(s, rule):
    if s.fold is not None:
        return False                                   # offering side, still on two walks
    if rule == "strict":
        return s.held == frozenset({s.send})
    return s.held.issubset(s.answered.union({s.send}))


def send(state, i, perm, rule):
    W, sides, fly, draws = state
    s = sides[i]
    pend, offer = s.pend, None
    first = s.lastw != W
    if first and pend is not None and pend[1] < W:
        pend = None                                    # an offer of an earlier window is replaced
    # a letter carries one heal part (its ninth field, MTPipeKey.swift:771-779): the answer owed first
    if first and pend is None and not s.owe and draws < DRAWS and can_offer(s, rule):
        key = perm[draws]
        draws += 1
        pend = (key, W, s.send)
        offer = (key, W)
    held, answered, answer, owe = s.held, s.answered, None, s.owe
    if owe:
        answer, owe = owe[0], owe[1:]
        h = healed(s.send, answer)                     # folded over the walk the answer travels under
        held, answered = held.union({h}), answered.union({h})
    spent = 0 if (answer is not None and offer is None) else 1
    s = s._replace(held=held, pend=pend, answered=answered, owe=owe, lastw=W, sent=s.sent + spent)
    sides = tuple(s if j == i else sides[j] for j in range(2))
    return (W, sides, tuple(sorted(fly + (Letter(i, s.send, offer, answer),), key=repr)), draws)


def deliver(state, n, stale, mutant, letgo):
    W, sides, fly, draws = state
    L = fly[n]
    fly = fly[:n] + fly[n + 1:]
    me = 1 - L.frm
    r = sides[me]
    if L.walk not in r.held:
        return (W, sides, fly, draws)                  # cannot open: lost
    if L.walk != r.send and (L.walk in r.answered or L.walk == r.fold):
        h = L.walk                                     # proof that the other side holds h
        r = r._replace(held=frozenset({h}), send=h, pend=None, fold=None, answered=frozenset())
    elif mutant == "early" and r.answered and L.walk == r.send:
        h = min(r.answered)                            # MUTANT: no proof asked
        r = r._replace(held=frozenset({h}), send=h, pend=None, fold=None, answered=frozenset())
    if L.answer is not None and r.pend is not None and (r.pend[0] == L.answer or mutant == "no-rho"):
        h = healed(r.pend[2], r.pend[0])               # the offering side folds over its own key
        r = r._replace(held=r.held.union({h}), send=h, fold=h, pend=None)
    # any other answer names a key this side does not hold, and is dropped
    if L.offer is not None and L.offer[0] not in r.answered_keys:
        key, win = L.offer
        p, take = r.pend, True
        if letgo != "off" and r.fold is not None:
            take = False                               # offering, still on two walks: ignored
        elif letgo == "later" and (r.answered or r.owe) and win <= r.awin:
            take = False                               # older than the offer answered: dropped
        elif letgo != "off" and (r.answered or r.owe):
            r = r._replace(held=frozenset(h for h in r.held if h not in r.answered),
                           answered=frozenset(), owe=())      # the unopened answer is let go
        crossing = p is not None and (p[1] == win or stale == "compare")
        if crossing:
            if p[0] < key:
                take = False                           # its own offer stands: it answers nothing
            else:
                r = r._replace(pend=None)              # it erases its key and answers the other
        elif p is not None and stale == "ignore":
            take = False
        if take:
            r = r._replace(owe=r.owe + (key,), answered_keys=r.answered_keys.union({key}),
                           awin=max(r.awin, win))
    sides = tuple(r if j == me else sides[j] for j in range(2))
    return (W, sides, fly, draws)


def successors(state, perm, rule, stale, mutant, letgo):
    W, sides, fly, draws = state
    if W < WMAX:
        yield "window %d" % (W + 1), (W + 1, sides, fly, draws)
    for i in range(2):
        if sides[i].sent < BUDGET or sides[i].owe:
            yield "send by %s" % "XY"[i], send(state, i, perm, rule)
    for n, L in enumerate(fly):
        yield "deliver %r" % (L,), deliver(state, n, stale, mutant, letgo)
        yield "drop %r" % (L,), (W, sides, fly[:n] + fly[n + 1:], draws)


def safe(state):
    a, b = state[1]
    return a.send in b.held and b.send in a.held


def one(state):
    for s in state[1]:
        bases = [h[1] for h in s.held if h != "w"]
        if len(bases) != len(set(bases)):
            return False
    return True


def path(parent, s):
    trace = []
    while parent[s] is not None:
        s, step = parent[s]
        trace.append(step)
    return trace[::-1]


def check(rule, stale, mutant=None, letgo="off"):
    explored, two = 0, None
    for perm in itertools.permutations(range(DRAWS)):
        s0 = start()
        parent = {s0: None}
        queue = deque([s0])
        while queue:
            s = queue.popleft()
            explored += 1
            if not safe(s):
                return explored, (perm, path(parent, s)), two
            if two is None and not one(s):
                two = (perm, path(parent, s))
            for step, t in successors(s, perm, rule, stale, mutant, letgo):
                if t not in parent:
                    parent[t] = (s, step)
                    queue.append(t)
    return explored, None, two


def show(title, found):
    perm, trace = found
    print("   %s (key order %s):" % (title, perm))
    for step in trace:
        print("     " + step)


def main():
    failed = False
    for letgo in ("off", "ios", "later"):
        for rule in ("strict", "loose"):
            for stale in ("compare", "answer", "ignore"):
                n, bad, two = check(rule, stale, letgo=letgo)
                print("LETGO=%-5s OFFER=%-6s STALE=%-7s %-6s ONE %-8s states %d" % (
                    letgo, rule, stale, "SAFE" if bad is None else "UNSAFE",
                    "holds" if two is None else "violated", n))
                if bad is not None:
                    failed = failed or letgo != "ios"
                    show("shortest run to a side the other cannot read", bad)
                if two is not None:
                    failed = failed or (letgo, rule, stale) == ("later", "strict", "compare")
                    show("shortest run to two healed walks from one old walk", two)
    for mutant in ("no-rho", "early"):
        n, bad, _ = check("strict", "compare", mutant)
        print("MUTANT %-6s %s states %d" % (mutant, "UNSAFE (found)" if bad else "SAFE (NOT FOUND)", n))
        if bad is None:
            failed = True
        else:
            show("the fault", bad)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
