/// What the Packet Monitor driver records about a packet besides its bytes.
///
/// The fields are the driver's `PACKET_MONITOR_STREAM_METADATA`, without its reserved field.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct PacketMetadata {
    /// Identifies the packet across the components it passes. Every report of one packet shares it.
    pub pkt_group_id: u64,
    /// How many reports the driver made for this packet group.
    pub pkt_count: u16,
    /// Which report of the group this one is.
    pub appearance_count: u16,
    /// The driver's direction code of the report.
    pub direction_name: u16,
    /// The driver's payload type code, the one `Packet::payload` is derived from.
    pub packet_type: u16,
    /// Component ID where the packet was captured. The same value as `Packet::component_id`.
    pub component_id: u16,
    /// The edge of the component the packet passed.
    pub edge_id: u16,
    /// The driver's reason code when the packet was dropped, otherwise zero.
    pub drop_reason: u32,
    /// The driver's code for where the packet was dropped, otherwise zero.
    pub drop_location: u32,
    /// The processor that handled the packet.
    pub processor: u16,
    /// The driver's timestamp of the report.
    pub timestamp: i64,
}
