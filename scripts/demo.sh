#!/usr/bin/env bash
# Build a demo vault and walk the whole Tier 0 surface.
set -euo pipefail

DAYDOC=${DAYDOC:-./target/release/daydoc}
VAULT=${1:-/tmp/daydoc-demo}
export DAYDOC_HOME=/tmp/daydoc-demo-state

rm -rf "$VAULT" "$DAYDOC_HOME"

"$DAYDOC" init "$VAULT"
export DAYDOC_VAULT="$VAULT"

echo "--- capture a decision"
"$DAYDOC" capture "We decided to adopt Okta for SSO across all products."

echo "--- capture something ambiguous (lands in inbox)"
"$DAYDOC" capture "hmm"

echo "--- a meeting transcript"
cat > "$VAULT/meetings/kickoff.md" <<'NOTE'
---
type: meeting
title: SSO kickoff
---
Alice: we need SSO before the enterprise pilot.
Bob: final call is to move billing to Razorpay instead of Stripe.
Carol: let's go with a 30 day pilot, owner: Priya
- [ ] Priya will prepare the rollout checklist
NOTE

"$DAYDOC" meeting "$VAULT/meetings/kickoff.md"
"$DAYDOC" meeting "$VAULT/meetings/kickoff.md" --approve 1 --actions

echo "--- supersede, then revert"
# DR-0002 came from the meeting approval, so the supersession takes DR-0003.
"$DAYDOC" supersede DR-0001 "Use Entra ID for SSO" --decision "Entra is cheaper at our volume."
"$DAYDOC" decisions
"$DAYDOC" revert DR-0003 --reason "wrong call, Okta stays"
"$DAYDOC" decisions

echo "--- ask"
"$DAYDOC" ask "what did we decide about okta"

echo "--- three notes with the same procedure propose an SOP"
for day in mon tue wed; do
  cat > "$VAULT/reference/rotation-$day.md" <<NOTE
---
type: reference
title: Key rotation $day
---
## Steps

1. open the console
2. rotate the signing key
3. restart the worker
4. post in the channel
NOTE
done
"$DAYDOC" sop

echo "--- stale, wrap, digest, handover"
"$DAYDOC" stale || true
"$DAYDOC" wrap --dry-run
"$DAYDOC" digest --dry-run
"$DAYDOC" handover --dry-run
"$DAYDOC" status
