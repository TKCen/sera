# EGRESS-ENFORCEMENT — Investigation and Design Document

## 1. Problem Statement

SERA routes all agent container outbound traffic through a Squid egress proxy (`sera-egress-proxy`) by
injecting `HTTP_PROXY` and `HTTPS_PROXY` environment variables into every container via
`ContainerSecurityMapper`. The proxy evaluates the container's `CapabilityPolicy` network allowlist and
blocks or forwards requests accordingly.

**This enforcement is advisory, not mandatory.**

Any process running inside an agent container can bypass the proxy by:

- Opening a raw TCP socket directly to an external IP (e.g. `socket.connect(443, "1.2.3.4")`)
- Using a library that reads proxy settings from a non-standard location or ignores them entirely
- Using a SOCKS proxy of its own construction
- Connecting over UDP (DNS, QUIC/HTTP3)

A BYOH (Bring Your Own Harness) container — one built by an operator rather than from SERA's
`sera-agent-worker` base image — may include arbitrary runtimes that do not honour
`HTTP_PROXY`/`HTTPS_PROXY`. An operator who imports a malicious or misconfigured template could exfiltrate
data or contact command-and-control infrastructure without any audit record.

This document investigates whether network-level enforcement is feasible and what SERA's V1 posture
should be.

---

## 2. Docker Networking Internals Relevant to Enforcement

### 2.1 How Docker Creates Networks

When `docker network create` creates a bridge network (the default driver), Docker:

1. Creates a Linux bridge interface (e.g. `br-a1b2c3d4e5f6`) on the host.
2. Attaches each container's `veth` pair to the bridge.
3. Adds NAT (`MASQUERADE`) rules in the `POSTROUTING` chain of `iptables nat` so containers can reach the
   internet via the host's default route.
4. Populates `DOCKER` and `DOCKER-ISOLATION-STAGE-1/2` chains in `iptables filter` to manage
   inter-network isolation.

The canonical hook for operator-supplied rules is the **`DOCKER-USER` chain**, which Docker guarantees to
call before its own `DOCKER` chain and to never flush on daemon restart.

### 2.2 iptables Chain Traversal for Outbound Container Traffic

For a packet originating in a container on `agent_net` destined for an external address:

```
PREROUTING (nat)  →  [routing decision: forward]  →  FORWARD (filter)
  └─ DOCKER-PREROUTING                                  └─ DOCKER-USER        ← operator rules go here
                                                         └─ DOCKER-ISOLATION-STAGE-1
                                                         └─ DOCKER
                                                         └─ FORWARD policy
                                          →  POSTROUTING (nat)
                                               └─ DOCKER-POSTROUTING (MASQUERADE)
```

Rules placed in `DOCKER-USER` are evaluated before Docker's own rules. A `DROP` in `DOCKER-USER` stops
the packet before it reaches `DOCKER` or the `FORWARD` default policy.

### 2.3 Identifying the agent_net Bridge Interface

```bash
# Get the agent_net network ID
AGENT_NET_ID=$(docker network inspect agent_net --format '{{.Id}}')

# The bridge interface name is br- followed by the first 12 chars of the network ID
BRIDGE_IF="br-${AGENT_NET_ID:0:12}"

# Verify
ip link show "$BRIDGE_IF"
```

The proxy container's IP on `agent_net` can be retrieved with:

```bash
PROXY_IP=$(docker inspect sera-egress-proxy \
  --format '{{(index .NetworkSettings.Networks "agent_net").IPAddress}}')
```

---

## 3. Enforcement Feasibility by Platform

### 3.1 Linux with Docker CE (feasible — recommended for production)

Docker CE runs the Docker daemon and the Linux kernel's netfilter directly on the host. Operators have
full access to `iptables`/`nftables` and can add rules to `DOCKER-USER`.

**Required rules for agent_net enforcement:**

```bash
#!/usr/bin/env bash
# scripts/egress-enforce.sh
# Applies network-level egress enforcement for agent containers.
# Run once after 'docker compose up -d'. Re-run after host reboot.

set -euo pipefail

NETWORK_NAME="${AGENT_NETWORK:-agent_net}"
PROXY_SERVICE="${PROXY_SERVICE:-sera-egress-proxy}"
PROXY_PORT="${PROXY_PORT:-3128}"

# Resolve runtime values
AGENT_NET_ID=$(docker network inspect "$NETWORK_NAME" --format '{{.Id}}')
BRIDGE_IF="br-${AGENT_NET_ID:0:12}"
PROXY_IP=$(docker inspect "$PROXY_SERVICE" \
  --format "{{(index .NetworkSettings.Networks \"$NETWORK_NAME\").IPAddress}}")

echo "Applying egress rules: bridge=$BRIDGE_IF proxy=$PROXY_IP:$PROXY_PORT"

# Flush any existing SERA-managed rules for idempotency
iptables -D DOCKER-USER -i "$BRIDGE_IF" -j SERA-EGRESS 2>/dev/null || true
iptables -F SERA-EGRESS 2>/dev/null || true
iptables -X SERA-EGRESS 2>/dev/null || true

# Create a named chain for easier inspection and teardown
iptables -N SERA-EGRESS

# Rule 1: Allow traffic from agent_net to the egress proxy (port 3128)
iptables -A SERA-EGRESS -d "$PROXY_IP" -p tcp --dport "$PROXY_PORT" -j ACCEPT

# Rule 2: Allow intra-agent_net traffic (containers talking to each other)
iptables -A SERA-EGRESS -o "$BRIDGE_IF" -j ACCEPT

# Rule 3: Allow established/related return traffic
iptables -A SERA-EGRESS -m conntrack --ctstate ESTABLISHED,RELATED -j ACCEPT

# Rule 4: Drop all other outbound traffic from agent_net
iptables -A SERA-EGRESS -j DROP

# Jump from DOCKER-USER into SERA-EGRESS for packets entering from agent_net bridge
iptables -I DOCKER-USER -i "$BRIDGE_IF" -j SERA-EGRESS

echo "Egress enforcement active."
```

**DNS considerations:** Containers use Docker's internal DNS resolver at `127.0.0.11:53`. DNS queries from
containers go to the Docker daemon's embedded resolver, not to external DNS directly. The rules above
allow intra-bridge traffic which covers DNS. However, if an agent attempts to bypass by querying an
external DNS server over UDP (port 53), add:

```bash
# Block external UDP DNS (agents must use Docker's embedded resolver)
iptables -I SERA-EGRESS -p udp --dport 53 ! -d 127.0.0.11 -j DROP
```

**Persistence across reboots:** `iptables` rules are not persistent by default. Options:

- `iptables-persistent` / `netfilter-persistent` (Debian/Ubuntu): `iptables-save > /etc/iptables/rules.v4`
- systemd unit that runs `egress-enforce.sh` after `docker.service` starts
- Docker Compose post-up hook (not natively supported — use a wrapper script)

The recommended approach is a systemd unit:

```ini
# /etc/systemd/system/sera-egress-enforce.service
[Unit]
Description=SERA egress enforcement rules
After=docker.service
Requires=docker.service
PartOf=docker.service

[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart=/opt/sera/scripts/egress-enforce.sh
ExecStop=/opt/sera/scripts/egress-enforce-teardown.sh

[Install]
WantedBy=multi-user.target
```

**nftables alternative (kernel 5.2+):**

Modern Linux distributions default to nftables with an iptables compatibility shim. If the host uses
native nftables, add rules to the `docker-user` chain in the `filter` table:

```nftables
table ip filter {
    chain docker-user {
        iifname "br-<agent_net_id>" goto sera-egress
    }

    chain sera-egress {
        ip daddr <proxy_ip> tcp dport 3128 accept
        oifname "br-<agent_net_id>" accept
        ct state established,related accept
        drop
    }
}
```

### 3.2 Docker Desktop on Windows (not feasible without a sidecar)

Docker Desktop on Windows runs all containers inside a lightweight Linux VM (based on WSL2). The network
topology is:

```
Windows host
  └─ WSL2 VM (docker-desktop distro)
       └─ Docker bridge (br-<id>)
            └─ container veth pairs
```

The Docker daemon, iptables, and the bridge interfaces all exist **inside the WSL2 VM**, not on the
Windows host. This means:

- You cannot run `iptables` from PowerShell or from a standard WSL2 distro to affect Docker's network
  namespace.
- `wsl -d docker-desktop` provides a shell inside the VM, but it is read-only and ephemeral — rules are
  lost on Docker Desktop restart.
- There is no supported API for injecting persistent iptables rules into the Docker Desktop VM.

**Conclusion: Direct iptables enforcement is NOT feasible on Docker Desktop.** The sidecar approach
(Section 4, Option A) is the only cross-platform enforcement mechanism.

### 3.3 Docker Desktop on macOS

Identical limitation to Windows. Docker Desktop uses a linuxkit VM (Apple Hypervisor on Apple Silicon,
QEMU on Intel). The kernel netfilter stack is inside the VM and inaccessible from the macOS host.

### 3.4 Kubernetes (out of V1 scope)

Kubernetes provides `NetworkPolicy` resources at the API level. Enforcement is delegated to the CNI
plugin (Calico, Cilium, Weave, etc.). A `NetworkPolicy` that restricts egress from the `agent` pod
namespace to only the egress proxy service would achieve network-level enforcement without host iptables
access. This is the correct approach for Kubernetes deployments but is outside SERA's current V1 scope.

---

## 4. Implementation Options

### Option A: Sidecar Enforcer Container (recommended for cross-platform production)

A privileged sidecar container with `NET_ADMIN` capability can configure iptables rules inside the Docker
network from within the VM/host where Docker is running. Because the sidecar is itself a container, it
runs inside the same environment as all other containers, regardless of whether Docker is running on bare
Linux or inside a VM.

The sidecar shares the network namespace with the egress proxy container so it can configure rules that
apply to the bridge:

```yaml
# docker-compose.prod.yaml (excerpt — add alongside docker-compose.yaml)
services:
  sera-egress-enforcer:
    image: alpine:3.20
    cap_add:
      - NET_ADMIN
    network_mode: 'service:sera-egress-proxy'
    restart: unless-stopped
    depends_on:
      sera-egress-proxy:
        condition: service_started
    entrypoint:
      - sh
      - -c
      - |
        set -e
        apk add --no-cache iptables iproute2

        # Discover bridge interface for agent_net from inside the container
        # The container sees the host bridge via its default route
        BRIDGE_IP=$(ip route | awk '/default/ {print $3}')

        # Allow only traffic destined for this container (the proxy) on port 3128
        # and drop everything else coming from agent_net
        iptables -I FORWARD -i eth0 -d "$BRIDGE_IP" -p tcp --dport 3128 -j ACCEPT
        iptables -I FORWARD -i eth0 -j DROP

        echo "Egress enforcement sidecar active. Bridge gateway: $BRIDGE_IP"
        exec sleep infinity
```

**Limitations of the sidecar approach:**

- `NET_ADMIN` is a significant capability — it allows the sidecar to modify any network interface it can
  reach, not just the agent bridge. The sidecar should run a minimal, pinned image.
- The sidecar must start before any agent containers. Use `depends_on` with `condition: service_healthy`
  on agent containers if needed.
- Rule logic inside the sidecar is more complex than the host script because the network namespace view
  differs.

### Option B: Host-level Script (Linux CE only)

`scripts/egress-enforce.sh` as shown in Section 3.1. Requires operator action after every host reboot.
Suitable for production bare-metal or VM deployments where the operator controls the host.

Provide a companion teardown script:

```bash
#!/usr/bin/env bash
# scripts/egress-enforce-teardown.sh
NETWORK_NAME="${AGENT_NETWORK:-agent_net}"
AGENT_NET_ID=$(docker network inspect "$NETWORK_NAME" --format '{{.Id}}' 2>/dev/null || echo "")
if [ -n "$AGENT_NET_ID" ]; then
  BRIDGE_IF="br-${AGENT_NET_ID:0:12}"
  iptables -D DOCKER-USER -i "$BRIDGE_IF" -j SERA-EGRESS 2>/dev/null || true
fi
iptables -F SERA-EGRESS 2>/dev/null || true
iptables -X SERA-EGRESS 2>/dev/null || true
echo "Egress enforcement rules removed."
```

### Option C: Advisory Enforcement (current V1 default)

`HTTP_PROXY`/`HTTPS_PROXY` environment variables are injected. Compliance relies on process behaviour.
Violations are detectable after the fact via `EgressLogWatcher` audit records, but not prevented.

This is the only option that requires zero additional host configuration and works identically on all
platforms.

---

## 5. Current Decision for V1

**V1 ships with Option C (advisory enforcement) as the default.**

Rationale:

- SERA targets Docker Desktop as the primary local development environment. Network-level enforcement
  is not feasible there without a sidecar that adds operational complexity.
- The agent-runtime base image (`sera-agent-worker`) honours `HTTP_PROXY`/`HTTPS_PROXY` because all
  HTTP calls go through standard Node.js/bun fetch which reads these variables. First-party containers
  are compliant.
- The primary BYOH risk vector is operator-imported templates. SERA's trust model already requires
  operator approval for template imports. Operators accepting BYOH templates accept the advisory
  enforcement limitation, documented explicitly in `docs/BYOH-CONTRACT.md`.
- `EgressLogWatcher` provides post-hoc audit coverage for Squid-proxied traffic. Violations that bypass
  the proxy produce no Squid log entry — the absence of a record for an outbound connection is itself a
  signal that can be detected by comparing observed network flows against audit records in a future
  security epic.

**Option A (sidecar) is provided as a commented-out configuration in `docker-compose.prod.yaml`** for
operators who require network-level enforcement in production Linux deployments.

**Option B (host script) is provided in `scripts/egress-enforce.sh`** with a README note that it
requires a post-startup invocation on Linux CE hosts.

---

## 6. Negative Compliance Test Design

### 6.1 Test Purpose

Verify that when network-level enforcement is active, a container on `agent_net` cannot make direct TCP
connections to external addresses without going through the egress proxy.

### 6.2 Test Container

```dockerfile
# test/egress/Dockerfile
FROM alpine:3.20
RUN apk add --no-cache curl netcat-openbsd
```

Add a test service to a `docker-compose.test.yaml`:

```yaml
services:
  sera-egress-test:
    build: test/egress/
    networks:
      - agent_net
    entrypoint: ['sleep', 'infinity']
```

### 6.3 Test Cases

**Test 1 — Direct HTTP to external IP (should fail with enforcement, succeed without):**

```bash
docker exec sera-egress-test \
  curl --max-time 5 --noproxy '*' http://93.184.216.34/ \
  && echo "BYPASS: enforcement not active" \
  || echo "BLOCKED: enforcement active"
```

`--noproxy '*'` forces curl to ignore `HTTP_PROXY`, simulating a non-compliant process.

**Test 2 — Direct HTTPS to external IP (should fail with enforcement):**

```bash
docker exec sera-egress-test \
  curl --max-time 5 --noproxy '*' https://93.184.216.34/ \
  && echo "BYPASS" \
  || echo "BLOCKED"
```

**Test 3 — Proxy-compliant request (should succeed on all platforms):**

```bash
docker exec sera-egress-test \
  curl --max-time 10 \
  --proxy "http://$PROXY_IP:3128" \
  http://example.com/ \
  && echo "PROXY: request succeeded" \
  || echo "FAIL: proxy request failed"
```

**Test 4 — DNS bypass attempt (should fail with enforcement when external DNS is blocked):**

```bash
docker exec sera-egress-test \
  nc -u -w 3 8.8.8.8 53 <<< "" \
  && echo "DNS BYPASS possible" \
  || echo "DNS BYPASS blocked"
```

### 6.4 Platform Skip Logic

In automated test suites, detect enforcement capability before running negative tests:

```bash
# scripts/check-egress-enforcement.sh
# Returns exit code 0 if enforcement is active, 1 if advisory only.

if docker exec sera-egress-test \
     curl --max-time 3 --noproxy '*' --silent http://93.184.216.34/ \
     > /dev/null 2>&1; then
  echo "ADVISORY: egress enforcement not active (Docker Desktop or rules not applied)"
  exit 1
else
  echo "ENFORCED: egress enforcement is active"
  exit 0
fi
```

Use this in CI:

```yaml
# .github/workflows/egress-test.yaml (excerpt)
- name: Check egress enforcement
  id: egress_check
  run: bash scripts/check-egress-enforcement.sh || echo "skip=true" >> "$GITHUB_OUTPUT"

- name: Negative compliance test
  if: steps.egress_check.outputs.skip != 'true'
  run: bash scripts/egress-negative-test.sh
```

On Docker Desktop (Windows CI runners, macOS runners), the negative compliance test is automatically
skipped and the skip is documented in the test output — it does not count as a failure.

---

## 7. Security Limitations Summary

| Threat                              | Mitigation                         | V1 Status                           |
| ----------------------------------- | ---------------------------------- | ----------------------------------- |
| HTTP library ignores proxy env vars | iptables enforcement (Options A/B) | Advisory only on Docker Desktop     |
| Raw TCP socket bypasses proxy       | iptables enforcement (Options A/B) | Advisory only on Docker Desktop     |
| UDP exfiltration (DNS, QUIC)        | iptables UDP rules + enforcement   | Advisory only on Docker Desktop     |
| BYOH container ignores proxy        | Operator trust requirement + audit | Documented in BYOH-CONTRACT.md      |
| Container escapes to host network   | Not connected to `host` network    | Enforced by compose network config  |
| Proxy itself is compromised         | Proxy runs in isolated `agent_net` | Squid image pinned, no shell access |

Network-level enforcement is a **defence-in-depth** control. The primary security controls for SERA
agents are:

1. **Capability policies** evaluated at the proxy — the proxy enforces allowlists per agent.
2. **Audit trail** — `EgressLogWatcher` records all proxied requests to the audit log.
3. **Sandbox isolation** — agent containers have no access to `sera_net` (core API) except via
   `sera_net` attachment for the chat endpoint.
4. **Token and resource budgets** — metering limits the blast radius of a misbehaving agent.

Network-level iptables enforcement is a complementary control that closes the raw-socket bypass gap. It
is not a replacement for the above controls.

---

## 8. Future Work

- **Epic: Network-Level Egress Enforcement** — Implement Option A sidecar as a supported production
  configuration with automated setup, health checks, and CI negative compliance tests on a Linux runner.
- **QUIC/HTTP3 blocking** — Block UDP port 443 at the bridge level to prevent QUIC-based bypasses until
  Squid adds native QUIC support.
- **eBPF enforcement** — Cilium or a custom eBPF program could enforce egress policy without iptables,
  with per-container granularity and lower overhead. Relevant for Kubernetes deployments.
- **Proxy authentication** — Add per-container proxy authentication (Basic auth with a per-instance token)
  so the proxy can associate requests with a specific agent instance rather than relying on source IP.
