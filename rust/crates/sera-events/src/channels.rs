//! Centrifugo channel namespace helpers.

/// Channel namespace builder for agent and system channels.
pub struct ChannelNamespace;

impl ChannelNamespace {
    /// Agent main channel for lifecycle and output events.
    pub fn agent_channel(instance_id: &str) -> String {
        format!("agent:{}", instance_id)
    }

    /// Agent thoughts/reasoning channel.
    pub fn agent_thoughts(instance_id: &str) -> String {
        format!("agent:{}:thoughts", instance_id)
    }

    /// Agent token consumption channel (metering).
    pub fn agent_tokens(instance_id: &str) -> String {
        format!("agent:{}:tokens", instance_id)
    }

    /// Internal agent container communication channel.
    pub fn internal_agent(instance_id: &str) -> String {
        format!("internal:agent:{}", instance_id)
    }

    /// System-wide broadcast channel.
    pub fn broadcast_system() -> String {
        "broadcast:system".to_string()
    }

    /// Circle broadcast channel.
    pub fn broadcast_circle(circle_id: &str) -> String {
        format!("broadcast:circle:{}", circle_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_channel_name() {
        assert_eq!(ChannelNamespace::agent_channel("inst-1"), "agent:inst-1");
        assert_eq!(ChannelNamespace::agent_channel("abc-xyz"), "agent:abc-xyz");
    }

    #[test]
    fn agent_thoughts_channel() {
        assert_eq!(
            ChannelNamespace::agent_thoughts("inst-1"),
            "agent:inst-1:thoughts"
        );
    }

    #[test]
    fn agent_tokens_channel() {
        assert_eq!(
            ChannelNamespace::agent_tokens("inst-1"),
            "agent:inst-1:tokens"
        );
    }

    #[test]
    fn internal_agent_channel() {
        assert_eq!(
            ChannelNamespace::internal_agent("inst-1"),
            "internal:agent:inst-1"
        );
    }

    #[test]
    fn broadcast_system_channel() {
        assert_eq!(ChannelNamespace::broadcast_system(), "broadcast:system");
    }

    #[test]
    fn broadcast_circle_channel() {
        assert_eq!(
            ChannelNamespace::broadcast_circle("circle-1"),
            "broadcast:circle:circle-1"
        );
    }

    // ------------------------------------------------------------------
    // All builders accept empty string without panicking
    // ------------------------------------------------------------------
    #[test]
    fn channel_builders_accept_empty_id() {
        assert_eq!(ChannelNamespace::agent_channel(""), "agent:");
        assert_eq!(ChannelNamespace::agent_thoughts(""), "agent::thoughts");
        assert_eq!(ChannelNamespace::agent_tokens(""), "agent::tokens");
        assert_eq!(ChannelNamespace::internal_agent(""), "internal:agent:");
        assert_eq!(ChannelNamespace::broadcast_circle(""), "broadcast:circle:");
    }

    // ------------------------------------------------------------------
    // Channel names with special characters (UUIDs, hyphens, underscores)
    // ------------------------------------------------------------------
    #[test]
    fn channel_builders_accept_uuid_style_ids() {
        let id = "550e8400-e29b-41d4-a716-446655440000";
        assert_eq!(
            ChannelNamespace::agent_channel(id),
            format!("agent:{}", id)
        );
        assert_eq!(
            ChannelNamespace::agent_thoughts(id),
            format!("agent:{}:thoughts", id)
        );
        assert_eq!(
            ChannelNamespace::agent_tokens(id),
            format!("agent:{}:tokens", id)
        );
        assert_eq!(
            ChannelNamespace::internal_agent(id),
            format!("internal:agent:{}", id)
        );
        assert_eq!(
            ChannelNamespace::broadcast_circle(id),
            format!("broadcast:circle:{}", id)
        );
    }

    // ------------------------------------------------------------------
    // Distinct instance IDs always produce distinct channel names
    // ------------------------------------------------------------------
    #[test]
    fn distinct_ids_produce_distinct_channels() {
        let ids = ["inst-a", "inst-b", "inst-c"];
        let channels: Vec<String> = ids.iter().map(|id| ChannelNamespace::agent_channel(id)).collect();
        // All unique
        let mut deduped = channels.clone();
        deduped.sort();
        deduped.dedup();
        assert_eq!(channels.len(), deduped.len());
    }

    // ------------------------------------------------------------------
    // broadcast_system is constant (no args)
    // ------------------------------------------------------------------
    #[test]
    fn broadcast_system_is_stable() {
        assert_eq!(ChannelNamespace::broadcast_system(), ChannelNamespace::broadcast_system());
    }
}
