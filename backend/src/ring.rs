use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};

pub const VIRTUAL_REPLICAS: usize = 64;
const SAMPLE_BENCHMARK_KEYS: usize = 2500;

/// 64-bit FNV-1a hash with avalanche finalizer for uniform full-ring key distribution.
#[inline]
pub fn fnv1a_64(bytes: &[u8]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;
    let mut hash = FNV_OFFSET_BASIS;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    // Murmur3 64-bit avalanche mix so short prefixed/suffixed keys span the entire 2^64 ring uniformly
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(0xff51afd7ed558ccd);
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(0xc4ceb9fe1a85ec53);
    hash ^= hash >> 33;
    hash
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeDescriptor {
    pub node_id: String,
    pub region: String,
    pub online: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterNodeStatus {
    pub node_id: String,
    pub region: String,
    pub online: bool,
    pub vnodes: usize,
    pub key_share_pct: f64,
    pub routed_requests: u64,
    pub primary_angle_deg: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RingSegment {
    pub node_id: String,
    pub start_deg: f64,
    pub end_deg: f64,
    pub hash_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterTopologySnapshot {
    pub hash_algorithm: String,
    pub virtual_replicas_per_node: usize,
    pub total_vnodes: usize,
    pub online_nodes: usize,
    pub total_nodes: usize,
    pub last_rebalance_pct: f64,
    pub last_toggled_node: Option<String>,
    pub nodes: Vec<ClusterNodeStatus>,
    pub segments: Vec<RingSegment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopologyToggleResult {
    pub node_id: String,
    pub online: bool,
    pub reassigned_keys: usize,
    pub sample_size: usize,
    pub reassigned_pct: f64,
    pub topology: ClusterTopologySnapshot,
}

struct RingInner {
    nodes: Vec<NodeDescriptor>,
    ring: BTreeMap<u64, String>,
    last_rebalance_pct: f64,
    last_toggled_node: Option<String>,
}

pub struct ConsistentHashRing {
    inner: RwLock<RingInner>,
    routed_counts: DashMap<String, AtomicU64>,
}

impl Default for ConsistentHashRing {
    fn default() -> Self {
        Self::new()
    }
}

impl ConsistentHashRing {
    pub fn new() -> Self {
        let default_nodes = vec![
            NodeDescriptor {
                node_id: "edge-iad-01".to_string(),
                region: "us-east-1".to_string(),
                online: true,
            },
            NodeDescriptor {
                node_id: "edge-gru-02".to_string(),
                region: "sa-east-1".to_string(),
                online: true,
            },
            NodeDescriptor {
                node_id: "edge-bog-03".to_string(),
                region: "sa-north-1".to_string(),
                online: true,
            },
            NodeDescriptor {
                node_id: "edge-fra-04".to_string(),
                region: "eu-central-1".to_string(),
                online: true,
            },
        ];

        let routed_counts = DashMap::new();
        for n in &default_nodes {
            routed_counts.insert(n.node_id.clone(), AtomicU64::new(0));
        }

        let ring = Self::build_ring(&default_nodes);

        Self {
            inner: RwLock::new(RingInner {
                nodes: default_nodes,
                ring,
                last_rebalance_pct: 0.0,
                last_toggled_node: None,
            }),
            routed_counts,
        }
    }

    fn build_ring(nodes: &[NodeDescriptor]) -> BTreeMap<u64, String> {
        let mut ring = BTreeMap::new();
        for node in nodes.iter().filter(|n| n.online) {
            for replica in 0..VIRTUAL_REPLICAS {
                let vkey = format!("{}#vnode-{:03}", node.node_id, replica);
                let hash = fnv1a_64(vkey.as_bytes());
                ring.insert(hash, node.node_id.clone());
            }
        }
        ring
    }

    fn lookup_in_map(ring: &BTreeMap<u64, String>, key: &str) -> (String, u64) {
        let key_hash = fnv1a_64(key.as_bytes());
        if ring.is_empty() {
            return ("unassigned".to_string(), key_hash);
        }
        if let Some((_, node_id)) = ring.range(key_hash..).next() {
            (node_id.clone(), key_hash)
        } else {
            let (_, node_id) = ring.iter().next().expect("non-empty ring has first entry");
            (node_id.clone(), key_hash)
        }
    }

    /// Routes a `client_key` to a physical cluster node and increments that node's counter.
    pub fn route_key(&self, key: &str) -> (String, u64) {
        let (node_id, key_hash) = {
            let guard = self.inner.read();
            Self::lookup_in_map(&guard.ring, key)
        };
        if let Some(counter) = self.routed_counts.get(&node_id) {
            counter.fetch_add(1, Ordering::Relaxed);
        }
        (node_id, key_hash)
    }

    /// Pure deterministic routing without mutating telemetry counters.
    pub fn preview_route(&self, key: &str) -> (String, u64) {
        let guard = self.inner.read();
        Self::lookup_in_map(&guard.ring, key)
    }

    /// Toggles a node online/offline and measures the exact percentage of sample keys reassigned.
    pub fn toggle_node(&self, node_id: &str) -> Result<TopologyToggleResult, String> {
        let (new_online, reassigned_keys, reassigned_pct) = {
            let mut guard = self.inner.write();
            let idx = guard
                .nodes
                .iter()
                .position(|n| n.node_id == node_id)
                .ok_or_else(|| format!("Cluster node '{}' not found", node_id))?;

            let currently_online = guard.nodes[idx].online;
            let active_count = guard.nodes.iter().filter(|n| n.online).count();
            if currently_online && active_count <= 1 {
                return Err("Cannot take the last remaining active edge node offline".to_string());
            }

            let before_assignments: Vec<String> = (0..SAMPLE_BENCHMARK_KEYS)
                .map(|i| {
                    let key = format!("bench-client-{}", i);
                    Self::lookup_in_map(&guard.ring, &key).0
                })
                .collect();

            let next_state = !currently_online;
            guard.nodes[idx].online = next_state;
            let updated_ring = Self::build_ring(&guard.nodes);

            let mut moved = 0usize;
            for (i, before_node) in before_assignments.iter().enumerate() {
                let key = format!("bench-client-{}", i);
                let (after_node, _) = Self::lookup_in_map(&updated_ring, &key);
                if &after_node != before_node {
                    moved += 1;
                }
            }

            let pct = ((moved as f64 / SAMPLE_BENCHMARK_KEYS as f64) * 10000.0).round() / 100.0;
            guard.ring = updated_ring;
            guard.last_rebalance_pct = pct;
            guard.last_toggled_node = Some(node_id.to_string());

            (next_state, moved, pct)
        };

        Ok(TopologyToggleResult {
            node_id: node_id.to_string(),
            online: new_online,
            reassigned_keys,
            sample_size: SAMPLE_BENCHMARK_KEYS,
            reassigned_pct,
            topology: self.get_topology(),
        })
    }

    pub fn get_topology(&self) -> ClusterTopologySnapshot {
        let guard = self.inner.read();
        let total_vnodes = guard.ring.len();
        let max_u64 = u64::MAX as f64;

        // Calculate theoretical arc share per node across the 2^64 hash ring
        let mut share_map: HashMap<String, f64> = HashMap::new();
        let mut vnode_counts: HashMap<String, usize> = HashMap::new();
        let mut segments: Vec<RingSegment> = Vec::with_capacity(guard.ring.len());

        let entries: Vec<(u64, String)> = guard
            .ring
            .iter()
            .map(|(h, id)| (*h, id.clone()))
            .collect();

        for (i, (hash, node_id)) in entries.iter().enumerate() {
            *vnode_counts.entry(node_id.clone()).or_insert(0) += 1;
            let next_hash = if i + 1 < entries.len() {
                entries[i + 1].0
            } else {
                entries[0].0
            };

            let arc_len = if next_hash > *hash {
                (next_hash - *hash) as f64
            } else {
                ((u64::MAX - *hash) as f64) + (next_hash as f64) + 1.0
            };
            let arc_pct = (arc_len / max_u64) * 100.0;
            *share_map.entry(node_id.clone()).or_insert(0.0) += arc_pct;

            let start_deg = (*hash as f64 / max_u64) * 360.0;
            let end_deg = if i + 1 < entries.len() {
                (next_hash as f64 / max_u64) * 360.0
            } else {
                360.0
            };

            segments.push(RingSegment {
                node_id: node_id.clone(),
                start_deg: (start_deg * 100.0).round() / 100.0,
                end_deg: (end_deg * 100.0).round() / 100.0,
                hash_hex: format!("{:016x}", hash),
            });
        }

        let nodes: Vec<ClusterNodeStatus> = guard
            .nodes
            .iter()
            .enumerate()
            .map(|(idx, desc)| {
                let routed_requests = self
                    .routed_counts
                    .get(&desc.node_id)
                    .map(|c| c.load(Ordering::Relaxed))
                    .unwrap_or(0);
                let vnodes = *vnode_counts.get(&desc.node_id).unwrap_or(&0);
                let share = (*share_map.get(&desc.node_id).unwrap_or(&0.0) * 100.0).round() / 100.0;
                ClusterNodeStatus {
                    node_id: desc.node_id.clone(),
                    region: desc.region.clone(),
                    online: desc.online,
                    vnodes,
                    key_share_pct: share,
                    routed_requests,
                    primary_angle_deg: (idx as f64) * 90.0 + 45.0,
                }
            })
            .collect();

        let online_nodes = nodes.iter().filter(|n| n.online).count();

        ClusterTopologySnapshot {
            hash_algorithm: "FNV-1a 64-bit".to_string(),
            virtual_replicas_per_node: VIRTUAL_REPLICAS,
            total_vnodes,
            online_nodes,
            total_nodes: nodes.len(),
            last_rebalance_pct: guard.last_rebalance_pct,
            last_toggled_node: guard.last_toggled_node.clone(),
            nodes,
            segments,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consistent_hashing_is_deterministic_and_balanced() {
        let ring = ConsistentHashRing::new();
        let (n1, h1) = ring.preview_route("tenant-alpha-10.0.1.4");
        let (n2, h2) = ring.preview_route("tenant-alpha-10.0.1.4");
        assert_eq!(n1, n2);
        assert_eq!(h1, h2);

        let mut distribution: HashMap<String, usize> = HashMap::new();
        for i in 0..2000 {
            let (node, _) = ring.route_key(&format!("client-key-{}", i));
            *distribution.entry(node).or_insert(0) += 1;
        }
        assert_eq!(distribution.len(), 4);
        for (node, count) in &distribution {
            assert!(
                *count > 250 && *count < 800,
                "node {} skewed too heavily: {}",
                node,
                count
            );
        }
    }

    #[test]
    fn node_toggle_demonstrates_minimal_key_reassignment() {
        let ring = ConsistentHashRing::new();
        let res = ring.toggle_node("edge-bog-03").expect("toggle succeeds");
        assert!(!res.online);
        // Removing 1 of 4 nodes should reassign roughly ~25% of keys (definitely < 36%)
        assert!(
            res.reassigned_pct > 15.0 && res.reassigned_pct < 36.0,
            "expected ~25% key reassignment, got {}%",
            res.reassigned_pct
        );
        assert_eq!(res.topology.online_nodes, 3);

        // Bringing it back online restores 4 nodes with ~25% reassignment
        let restore = ring.toggle_node("edge-bog-03").expect("restore succeeds");
        assert!(restore.online);
        assert_eq!(restore.topology.online_nodes, 4);
    }

    #[test]
    fn cannot_disable_last_active_node() {
        let ring = ConsistentHashRing::new();
        ring.toggle_node("edge-iad-01").unwrap();
        ring.toggle_node("edge-gru-02").unwrap();
        ring.toggle_node("edge-bog-03").unwrap();
        let err = ring.toggle_node("edge-fra-04").unwrap_err();
        assert!(err.contains("last remaining"));
    }
}
