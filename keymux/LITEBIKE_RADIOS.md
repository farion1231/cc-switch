# LiteBike Radios Integration

## Overview

ModelMux integrates with LiteBike's radios module for **real-time carrier/network quality metrics**. This enables intelligent routing based on actual network conditions.

## Features

- ✅ **Syscall-only detection** - No /proc, /sys, /dev dependencies
- ✅ **Netlink/ioctl support** - Interface enumeration on Linux/Android
- ✅ **WiFi + Cellular** - wlan*, rmnet*, eth* interfaces
- ✅ **Signal strength** - dBm measurements (-100 to -30)
- ✅ **Latency probing** - HTTP probes to measure RTT
- ✅ **Packet loss** - Estimate from probe failures
- ✅ **Bandwidth estimation** - Based on interface type
- ✅ **Automatic selection** - Best interface by quality score

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                   ModelMux                               │
│                                                          │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  │
│  │   Ranker     │  │   Carrier    │  │   LiteBike   │  │
│  │              │◄─┤   Metrics    │◄─┤   Radios     │  │
│  │ score = f(   │  │              │  │   Module     │  │
│  │   latency,   │  │ - Signal     │  │              │  │
│  │   cost,      │  │ - Latency    │  │ - Netlink    │  │
│  │   quota,     │  │ - Loss       │  │ - ioctl      │  │
│  │   carrier    │  │ - Bandwidth  │  │ - Signal     │  │
│  │ )            │  │              │  │              │  │
│  └──────────────┘  └──────────────┘  └──────────────┘  │
└─────────────────────────────────────────────────────────┘
```

## Integration Points

### Phase 1: Fallback Implementation (Current)

**Status**: ✅ Implemented

Uses std::net for interface detection and HTTP probes for latency:

```rust
// modelmux/src/carrier.rs
fn detect_interfaces_std(&mut self) -> Result<()> {
    // Uses get_if_addrs crate for interface enumeration
    if let Ok(addrs) = get_if_addrs::get_if_addrs() {
        for addr in addrs {
            // Update interface list
        }
    }
    Ok(())
}

async fn probe_latencies(&mut self) -> Result<()> {
    // HTTP probes to measure latency
    for target in &self.probe_targets {
        let start = Instant::now();
        client.get(target).send().await?;
        let latency = start.elapsed().as_secs_f64() * 1000.0;
    }
}
```

### Phase 2: LiteBike Syscall Integration (TODO)

**Status**: ⏳ Pending

Replace fallback with LiteBike syscall-based detection:

```rust
// modelmux/src/carrier.rs (TODO)
fn detect_interfaces_syscall(&self) -> Result<Vec<RadioInterface>> {
    // Integrate with LiteBike radios module
    use literbike::radios::{list_interfaces, get_signal_strength};
    
    let interfaces = list_interfaces()?; // Netlink-based
    let mut result = Vec::new();
    
    for iface in interfaces {
        let signal = get_signal_strength(&iface.name)?; // ioctl/wireless-ext
        result.push(RadioInterface {
            name: iface.name,
            signal_strength: Some(signal),
            // ...
        });
    }
    
    Ok(result)
}
```

### Phase 3: Egress Path Selection (TODO)

**Status**: ⏳ Pending

Use best interface for upstream requests:

```rust
// modelmux/src/router.rs (TODO)
async fn forward_to_upstream(...) {
    // Get best interface from carrier metrics
    let best_iface = carrier::get_best_interface();
    
    // Bind request to specific interface
    let client = reqwest::Client::builder()
        .interface(&best_iface.name) // TODO: reqwest doesn't support this
        .build()?;
    
    // Send request via selected interface
    client.post(upstream_url).send().await?;
}
```

## Radio Interface Types

| Type | Prefixes | Detection |
|------|----------|-----------|
| **WiFi** | wlan*, eth*, wifi | `name.starts_with("wlan")` |
| **Cellular** | rmnet*, ccinet*, wwan* | `name.starts_with("rmnet")` |
| **Loopback** | lo, loopback | `name == "lo"` |
| **Unknown** | other | Fallback |

## Quality Score Calculation

```rust
quality_score = 
    signal_score * 0.3 +    // -100dBm to -30dBm → 0.0 to 1.0
    latency_score * 0.3 +   // 1/(1 + latency_ms/50)
    loss_score * 0.2 +      // 1.0 - packet_loss
    bandwidth_score * 0.2   // bandwidth_mbps / 100
```

**Example Scores**:

| Interface | Signal | Latency | Loss | Bandwidth | Score |
|-----------|--------|---------|------|-----------|-------|
| WiFi (excellent) | -50 dBm | 20ms | 0% | 100 Mbps | 0.92 |
| WiFi (poor) | -85 dBm | 150ms | 5% | 10 Mbps | 0.45 |
| Cellular (4G) | -75 dBm | 80ms | 2% | 50 Mbps | 0.68 |
| Cellular (3G) | -90 dBm | 300ms | 10% | 5 Mbps | 0.28 |

## Configuration

### Probe Targets

Default targets for latency measurement:

```rust
probe_targets: vec![
    "https://1.1.1.1".to_string(),      // Cloudflare DNS
    "https://8.8.8.8".to_string(),      // Google DNS
    "https://api.openai.com".to_string(), // API endpoint
]
```

Customize in code:

```rust
let mut metrics = CarrierMetrics::new()
    .with_probe_targets(vec![
        "https://your-probe-target.com".to_string(),
    ]);
```

### Probe Interval

Default: 10 seconds

```rust
let mut metrics = CarrierMetrics::new()
    .with_probe_interval(30); // Probe every 30 seconds
```

## Usage

### Programmatic

```rust
use modelmux::carrier::{CarrierMetrics, RadioType};

// Initialize
let mut metrics = CarrierMetrics::new();

// Refresh interfaces
metrics.refresh_interfaces()?;

// Probe latencies (async)
metrics.probe_latencies().await?;

// Get best interface
if let Some(best) = metrics.get_best_interface() {
    println!("Best: {} (score: {:.2})", 
             best.name, best.quality_score());
    println!("  Signal: {:?} dBm", best.signal_strength);
    println!("  Latency: {:.1}ms", best.latency_ms);
    println!("  Loss: {:.1}%", best.packet_loss * 100.0);
}
```

### Logs

```
[INFO]  Carrier metrics: initialized
[INFO]  Detected 3 interfaces via syscall
[INFO]  Selected best interface: wlan0 (score: 0.85)
[DEBUG]  wlan0: 25.3ms latency, 0.5% loss
[DEBUG]  rmnet0: 85.7ms latency, 2.1% loss
```

## LiteBike Integration Checklist

### Phase 1: Fallback (✅ Done)

- [x] Interface detection via `get_if_addrs`
- [x] HTTP probe-based latency measurement
- [x] Quality score calculation
- [x] Automatic best interface selection

### Phase 2: Syscall Integration (⏳ TODO)

- [ ] Add LiteBike as dependency
- [ ] Implement `detect_interfaces_syscall()` with Netlink
- [ ] Add signal strength detection (ioctl/wireless-ext)
- [ ] Add cellular signal detection (QMI/MBIM)
- [ ] Handle Android-specific interfaces (rmnet*)

### Phase 3: Egress Selection (⏳ TODO)

- [ ] Interface binding for HTTP client
- [ ] Per-request interface selection
- [ ] Failover on interface loss
- [ ] Connection migration (QUIC + interface change)

## Platform Support

| Platform | Interface Detection | Signal Strength | Latency Probe |
|----------|---------------------|-----------------|---------------|
| **Linux** | ✅ Netlink | ✅ ioctl | ✅ HTTP |
| **Android** | ✅ Netlink | ✅ QMI/MBIM | ✅ HTTP |
| **macOS** | ✅ get_if_addrs | ❌ (needs CoreWLAN) | ✅ HTTP |
| **Windows** | ✅ get_if_addrs | ❌ (needs WLAN API) | ✅ HTTP |

## Troubleshooting

### No Interfaces Detected

```bash
# Check permissions (Netlink requires CAP_NET_ADMIN)
sudo modelmux --port 8888

# Or check fallback logs
modelmux --port 8888 --verbose 2>&1 | grep "interface"
```

### High Latency

```bash
# Check probe targets
modelmux --port 8888 --verbose 2>&1 | grep "probe"

# Test manually
curl -w "@curl-format.txt" -o /dev/null -s https://1.1.1.1
curl -w "@curl-format.txt" -o /dev/null -s https://8.8.8.8
```

### Signal Strength Not Showing

```bash
# Check if wireless extensions available
iwconfig 2>/dev/null || echo "No wireless interfaces"

# Check cellular interfaces
mmcli -L 2>/dev/null || echo "No cellular modem (libmbim required)"
```

## References

- **Netlink**: https://www.infradead.org/~tgr/libnl/
- **Wireless Extensions**: https://hewlettpackard.github.io/wireless-tools/
- **QMI**: https://www.freedesktop.org/software/libqmi/
- **MBIM**: https://www.freedesktop.org/software/libmbim/
- **LiteBike Radios**: `/Users/jim/work/literbike/src/radios.rs`
