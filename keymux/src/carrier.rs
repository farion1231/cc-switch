//! Carrier/Radio metrics module
//!
//! Integrates with LiteBike radios module for real-time network quality metrics.
//! Falls back to simple HTTP probes when LiteBike is not available.
//!
//! Features:
//! - Syscall-only radio detection (no /proc, /sys, /dev)
//! - Netlink/ioctl for interface enumeration
//! - Carrier signal strength, latency, packet loss
//! - WiFi (wlan*) and cellular (rmnet*) interface support
//! - Automatic interface selection based on quality

use anyhow::{Result, Context};
use log::{debug, info, warn};
use std::collections::HashMap;
use std::net::IpAddr;
use std::time::{Duration, Instant};

/// Radio interface types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadioType {
    /// WiFi interface (wlan*, eth*)
    WiFi,
    /// Cellular interface (rmnet*, ccinet*)
    Cellular,
    /// Loopback (lo)
    Loopback,
    /// Unknown/other
    Unknown,
}

impl RadioType {
    /// Detect radio type from interface name
    pub fn from_name(name: &str) -> Self {
        if name.starts_with("wlan") || name.starts_with("eth") || name == "wifi" {
            RadioType::WiFi
        } else if name.starts_with("rmnet") || name.starts_with("ccinet") || name.starts_with("wwan") {
            RadioType::Cellular
        } else if name == "lo" || name.starts_with("loopback") {
            RadioType::Loopback
        } else {
            RadioType::Unknown
        }
    }
}

/// Radio interface information
#[derive(Debug, Clone)]
pub struct RadioInterface {
    /// Interface name (e.g., "wlan0", "rmnet0")
    pub name: String,
    /// Radio type
    pub radio_type: RadioType,
    /// IP address
    pub ip_address: Option<IpAddr>,
    /// Signal strength (dBm, higher is better, e.g., -50 > -100)
    pub signal_strength: Option<f64>,
    /// Current latency estimate (ms)
    pub latency_ms: f64,
    /// Packet loss estimate (0.0 - 1.0)
    pub packet_loss: f64,
    /// Estimated bandwidth (Mbps)
    pub bandwidth_mbps: f64,
    /// Is interface currently active
    pub is_active: bool,
    /// Last updated timestamp
    pub last_updated: Instant,
}

impl RadioInterface {
    /// Create a new radio interface with default values
    pub fn new(name: String) -> Self {
        Self {
            name,
            radio_type: RadioType::Unknown,
            ip_address: None,
            signal_strength: None,
            latency_ms: 100.0, // Default 100ms
            packet_loss: 0.0,
            bandwidth_mbps: 10.0, // Default 10 Mbps
            is_active: false,
            last_updated: Instant::now(),
        }
    }
    
    /// Calculate quality score (0.0 - 1.0, higher is better)
    pub fn quality_score(&self) -> f64 {
        if !self.is_active {
            return 0.0;
        }
        
        // Signal score (normalize dBm to 0-1)
        // Typical range: -100 dBm (bad) to -30 dBm (excellent)
        let signal_score = if let Some(strength) = self.signal_strength {
            ((strength + 100.0) / 70.0).clamp(0.0, 1.0)
        } else {
            0.5 // Unknown signal = medium score
        };
        
        // Latency score (lower is better)
        // < 50ms = excellent, > 500ms = poor
        let latency_score = 1.0 / (1.0 + self.latency_ms / 50.0);
        
        // Packet loss score (lower is better)
        let loss_score = 1.0 - self.packet_loss;
        
        // Bandwidth score (higher is better, cap at 100 Mbps)
        let bandwidth_score = (self.bandwidth_mbps / 100.0).clamp(0.0, 1.0);
        
        // Weighted average
        signal_score * 0.3 + latency_score * 0.3 + loss_score * 0.2 + bandwidth_score * 0.2
    }
}

/// Carrier metrics collector
pub struct CarrierMetrics {
    /// Known radio interfaces
    interfaces: HashMap<String, RadioInterface>,
    /// Currently selected best interface
    best_interface: Option<String>,
    /// Probe targets for latency measurement
    probe_targets: Vec<String>,
    /// Last probe time
    last_probe: Option<Instant>,
    /// Probe interval (seconds)
    probe_interval: u64,
}

impl CarrierMetrics {
    /// Create new carrier metrics collector
    pub fn new() -> Self {
        Self {
            interfaces: HashMap::new(),
            best_interface: None,
            probe_targets: vec![
                "https://1.1.1.1".to_string(), // Cloudflare DNS
                "https://8.8.8.8".to_string(), // Google DNS
                "https://api.openai.com".to_string(), // API endpoint
            ],
            last_probe: None,
            probe_interval: 10, // Probe every 10 seconds
        }
    }
    
    /// Set custom probe interval (seconds)
    pub fn with_probe_interval(mut self, seconds: u64) -> Self {
        self.probe_interval = seconds;
        self
    }
    
    /// Set custom probe targets
    pub fn with_probe_targets(mut self, targets: Vec<String>) -> Self {
        self.probe_targets = targets;
        self
    }
    
    /// Refresh radio interface list
    ///
    /// Tries LiteBike syscall-based detection first, falls back to std::net
    pub fn refresh_interfaces(&mut self) -> Result<()> {
        debug!("Refreshing radio interfaces...");
        
        // Try LiteBike-style syscall detection
        match self.detect_interfaces_syscall() {
            Ok(interfaces) => {
                for iface in interfaces {
                    self.interfaces.insert(iface.name.clone(), iface);
                }
                info!("Detected {} interfaces via syscall", self.interfaces.len());
            }
            Err(e) => {
                warn!("Syscall detection failed: {}, falling back to std::net", e);
                // Fallback to std::net detection
                self.detect_interfaces_std()?;
            }
        }
        
        // Update best interface
        self.update_best_interface();
        
        Ok(())
    }
    
    /// Detect interfaces using LiteBike-style syscalls (Netlink/ioctl)
    ///
    /// This is a placeholder - in production, integrate with actual LiteBike radios module
    fn detect_interfaces_syscall(&self) -> Result<Vec<RadioInterface>> {
        // TODO: Integrate with LiteBike radios module
        // For now, return error to trigger fallback
        // 
        // Production implementation would use:
        // - Netlink sockets for interface enumeration
        // - ioctl for interface details
        // - /sys/class/net for interface status (if available)
        // - Signal strength from wireless extensions (iwconfig)
        // - Cellular signal from QMI/MBIM (libqmi, libmbim)
        
        anyhow::bail!("LiteBike integration not yet implemented")
    }
    
    /// Detect interfaces using std::net (fallback)
    fn detect_interfaces_std(&mut self) -> Result<()> {
        // Get local addresses
        if let Ok(addrs) = get_if_addrs::get_if_addrs() {
            for addr in addrs {
                let name = addr.name.clone();
                let mut iface = self.interfaces
                    .entry(name.clone())
                    .or_insert_with(|| RadioInterface::new(name));
                
                iface.ip_address = Some(addr.ip());
                iface.is_active = !addr.is_loopback();
                iface.radio_type = RadioType::from_name(&addr.name);
                iface.last_updated = Instant::now();
            }
        }
        
        Ok(())
    }
    
    /// Probe interfaces for latency measurement
    pub async fn probe_latencies(&mut self) -> Result<()> {
        // Check if it's time to probe
        if let Some(last) = self.last_probe {
            if last.elapsed().as_secs() < self.probe_interval {
                return Ok(()); // Not yet time to probe
            }
        }
        
        debug!("Probing latencies...");
        
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()?;
        
        for (name, iface) in self.interfaces.iter_mut() {
            if !iface.is_active {
                continue;
            }
            
            // Probe each target
            let mut latencies = Vec::new();
            for target in &self.probe_targets {
                let start = Instant::now();
                match client.get(target).send().await {
                    Ok(_) => {
                        latencies.push(start.elapsed().as_secs_f64() * 1000.0); // Convert to ms
                    }
                    Err(_) => {
                        // Probe failed, count as packet loss
                        iface.packet_loss = (iface.packet_loss + 0.1).min(1.0);
                    }
                }
            }
            
            // Calculate average latency
            if !latencies.is_empty() {
                iface.latency_ms = latencies.iter().sum::<f64>() / latencies.len() as f64;
                // Reduce packet loss on successful probes
                iface.packet_loss = (iface.packet_loss * 0.9).max(0.0);
            }
            
            iface.last_updated = Instant::now();
            debug!("  {}: {:.1}ms latency, {:.1}% loss", 
                   name, iface.latency_ms, iface.packet_loss * 100.0);
        }
        
        self.last_probe = Some(Instant::now());
        
        // Update best interface after probing
        self.update_best_interface();
        
        Ok(())
    }
    
    /// Update best interface selection
    fn update_best_interface(&mut self) {
        let mut best_name: Option<String> = None;
        let mut best_score = 0.0;
        
        for (name, iface) in &self.interfaces {
            if !iface.is_active {
                continue;
            }
            
            let score = iface.quality_score();
            if score > best_score {
                best_score = score;
                best_name = Some(name.clone());
            }
        }
        
        if best_name != self.best_interface {
            if let Some(ref name) = best_name {
                info!("Selected best interface: {} (score: {:.2})", name, best_score);
            }
            self.best_interface = best_name;
        }
    }
    
    /// Get the best radio interface
    pub fn get_best_interface(&self) -> Option<&RadioInterface> {
        self.best_interface
            .as_ref()
            .and_then(|name| self.interfaces.get(name))
    }
    
    /// Get all interfaces
    pub fn get_interfaces(&self) -> &HashMap<String, RadioInterface> {
        &self.interfaces
    }
    
    /// Get interface by name
    pub fn get_interface(&self, name: &str) -> Option<&RadioInterface> {
        self.interfaces.get(name)
    }
    
    /// Convert to carrier metrics for ranker
    pub fn to_carrier_metrics(&self) -> crate::types::CarrierMetrics {
        if let Some(iface) = self.get_best_interface() {
            crate::types::CarrierMetrics {
                signal_strength: iface.signal_strength.unwrap_or(-70.0),
                latency_ms: iface.latency_ms,
                packet_loss: iface.packet_loss,
                bandwidth_mbps: iface.bandwidth_mbps,
            }
        } else {
            crate::types::CarrierMetrics::default()
        }
    }
}

impl Default for CarrierMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Global carrier metrics instance (lazy-initialized)
static mut CARRIER_METRICS: Option<CarrierMetrics> = None;

/// Initialize global carrier metrics
pub fn init_carrier_metrics() -> Result<()> {
    unsafe {
        CARRIER_METRICS = Some(CarrierMetrics::new());
    }
    Ok(())
}

/// Get global carrier metrics
pub fn get_carrier_metrics() -> Option<&'static CarrierMetrics> {
    unsafe { CARRIER_METRICS.as_ref() }
}

/// Get mutable global carrier metrics
pub fn get_carrier_metrics_mut() -> Option<&'static mut CarrierMetrics> {
    unsafe { CARRIER_METRICS.as_mut() }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_radio_type_from_name() {
        assert_eq!(RadioType::from_name("wlan0"), RadioType::WiFi);
        assert_eq!(RadioType::from_name("eth0"), RadioType::WiFi);
        assert_eq!(RadioType::from_name("rmnet0"), RadioType::Cellular);
        assert_eq!(RadioType::from_name("lo"), RadioType::Loopback);
        assert_eq!(RadioType::from_name("unknown0"), RadioType::Unknown);
    }
    
    #[test]
    fn test_radio_interface_quality_score() {
        let mut iface = RadioInterface::new("wlan0".to_string());
        iface.is_active = true;
        iface.signal_strength = Some(-50.0); // Excellent signal
        iface.latency_ms = 20.0; // Low latency
        iface.packet_loss = 0.0; // No loss
        iface.bandwidth_mbps = 100.0; // High bandwidth
        
        let score = iface.quality_score();
        assert!(score > 0.8, "Excellent interface should have high score, got {}", score);
        
        // Poor interface
        let mut poor_iface = RadioInterface::new("rmnet0".to_string());
        poor_iface.is_active = true;
        poor_iface.signal_strength = Some(-95.0); // Poor signal
        poor_iface.latency_ms = 300.0; // High latency
        poor_iface.packet_loss = 0.3; // 30% loss
        poor_iface.bandwidth_mbps = 1.0; // Low bandwidth
        
        let poor_score = poor_iface.quality_score();
        assert!(poor_score < 0.4, "Poor interface should have low score, got {}", poor_score);
        assert!(score > poor_score, "Excellent should be better than poor");
    }
}
