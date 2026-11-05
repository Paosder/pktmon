use super::c_api::PacketMonitorStreamMetadata;
use crate::PacketMetadata;

impl From<&PacketMonitorStreamMetadata> for PacketMetadata {
    fn from(metadata: &PacketMonitorStreamMetadata) -> Self {
        Self {
            pkt_group_id: metadata.pkt_group_id,
            pkt_count: metadata.pkt_count,
            appearance_count: metadata.appearance_count,
            direction_name: metadata.direction_name,
            packet_type: metadata.packet_type,
            component_id: metadata.component_id,
            edge_id: metadata.edge_id,
            drop_reason: metadata.drop_reason,
            drop_location: metadata.drop_location,
            processor: metadata.processor,
            timestamp: metadata.timestamp,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn driver_metadata() -> PacketMonitorStreamMetadata {
        PacketMonitorStreamMetadata {
            pkt_group_id: 0x0123_4567_89ab_cdef,
            pkt_count: 2,
            appearance_count: 1,
            direction_name: 3,
            packet_type: 5,
            component_id: 17,
            edge_id: 4,
            reserved: 0xffff,
            drop_reason: 0x0001_0203,
            drop_location: 0x0405_0607,
            processor: 9,
            timestamp: -42,
        }
    }

    #[test]
    fn every_field_the_driver_reports_is_carried_over() {
        assert_eq!(
            PacketMetadata::from(&driver_metadata()),
            PacketMetadata {
                pkt_group_id: 0x0123_4567_89ab_cdef,
                pkt_count: 2,
                appearance_count: 1,
                direction_name: 3,
                packet_type: 5,
                component_id: 17,
                edge_id: 4,
                drop_reason: 0x0001_0203,
                drop_location: 0x0405_0607,
                processor: 9,
                timestamp: -42,
            }
        );
    }

    #[test]
    fn the_extreme_values_are_not_narrowed() {
        let metadata = PacketMonitorStreamMetadata {
            pkt_group_id: u64::MAX,
            pkt_count: u16::MAX,
            appearance_count: u16::MAX,
            direction_name: u16::MAX,
            packet_type: u16::MAX,
            component_id: u16::MAX,
            edge_id: u16::MAX,
            reserved: 0,
            drop_reason: u32::MAX,
            drop_location: u32::MAX,
            processor: u16::MAX,
            timestamp: i64::MIN,
        };
        let converted = PacketMetadata::from(&metadata);
        assert_eq!(converted.pkt_group_id, u64::MAX);
        assert_eq!(converted.drop_reason, u32::MAX);
        assert_eq!(converted.drop_location, u32::MAX);
        assert_eq!(converted.timestamp, i64::MIN);
    }
}
