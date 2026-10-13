//! Open Core Router (OpenCR) — asl-router-netstack
//! Version: RINJANI_1_0_0
//! Standard: DO-178C DAL-A / ARINC 653 IMA
//! Minimal Native Carrier-Grade Network Protocol Stack (Ethernet II, ARP, IPv4, ICMP, UDP)

#![no_std]

// -----------------------------------------------------------------------------
// PROTOCOL CONSTANTS
// -----------------------------------------------------------------------------

pub const ETHERTYPE_IPV4: u16 = 0x0800;
pub const ETHERTYPE_ARP: u16 = 0x0806;

pub const IP_PROTO_ICMP: u8 = 1;
pub const IP_PROTO_TCP: u8 = 6;
pub const IP_PROTO_UDP: u8 = 17;

pub const ICMP_TYPE_ECHO_REPLY: u8 = 0;
pub const ICMP_TYPE_ECHO_REQUEST: u8 = 8;

pub const ARP_HTYPE_ETHERNET: u16 = 1;
pub const ARP_PTYPE_IPV4: u16 = 0x0800;
pub const ARP_OP_REQUEST: u16 = 1;
pub const ARP_OP_REPLY: u16 = 2;

// -----------------------------------------------------------------------------
// CHECKSUM UTILITY (RFC 1071 STANDARD 1'S COMPLEMENT)
// -----------------------------------------------------------------------------

pub fn compute_internet_checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i + 1 < data.len() {
        let word = u16::from_be_bytes([data[i], data[i + 1]]);
        sum = sum.wrapping_add(word as u32);
        i += 2;
    }
    if i < data.len() {
        let word = u16::from_be_bytes([data[i], 0]);
        sum = sum.wrapping_add(word as u32);
    }
    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !sum as u16
}

// -----------------------------------------------------------------------------
// PARSED PACKET STRUCTURES
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EthernetHeader {
    pub dst_mac: [u8; 6],
    pub src_mac: [u8; 6],
    pub ethertype: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArpPacket {
    pub htype: u16,
    pub ptype: u16,
    pub hlen: u8,
    pub plen: u8,
    pub opcode: u16,
    pub sender_mac: [u8; 6],
    pub sender_ip: [u8; 4],
    pub target_mac: [u8; 6],
    pub target_ip: [u8; 4],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ipv4Header {
    pub version: u8,
    pub ihl: u8,
    pub dscp_ecn: u8,
    pub total_length: u16,
    pub identification: u16,
    pub flags_fragment_offset: u16,
    pub ttl: u8,
    pub protocol: u8,
    pub header_checksum: u16,
    pub src_ip: [u8; 4],
    pub dst_ip: [u8; 4],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IcmpHeader {
    pub icmp_type: u8,
    pub code: u8,
    pub checksum: u16,
    pub rest_of_header: [u8; 4],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UdpHeader {
    pub src_port: u16,
    pub dst_port: u16,
    pub length: u16,
    pub checksum: u16,
}

// -----------------------------------------------------------------------------
// PARSERS
// -----------------------------------------------------------------------------

pub fn parse_ethernet_header(buf: &[u8]) -> Result<(EthernetHeader, &[u8]), &'static str> {
    if buf.len() < 14 {
        return Err("Ethernet frame too short (<14B)");
    }
    let mut dst_mac = [0u8; 6];
    let mut src_mac = [0u8; 6];
    dst_mac.copy_from_slice(&buf[0..6]);
    src_mac.copy_from_slice(&buf[6..12]);
    let ethertype = u16::from_be_bytes([buf[12], buf[13]]);
    Ok((
        EthernetHeader { dst_mac, src_mac, ethertype },
        &buf[14..],
    ))
}

pub fn parse_arp_packet(payload: &[u8]) -> Result<ArpPacket, &'static str> {
    if payload.len() < 28 {
        return Err("ARP packet too short (<28B)");
    }
    let htype = u16::from_be_bytes([payload[0], payload[1]]);
    let ptype = u16::from_be_bytes([payload[2], payload[3]]);
    let hlen = payload[4];
    let plen = payload[5];
    let opcode = u16::from_be_bytes([payload[6], payload[7]]);

    if hlen != 6 || plen != 4 {
        return Err("Unsupported ARP hardware or protocol length");
    }

    let mut sender_mac = [0u8; 6];
    let mut sender_ip = [0u8; 4];
    let mut target_mac = [0u8; 6];
    let mut target_ip = [0u8; 4];

    sender_mac.copy_from_slice(&payload[8..14]);
    sender_ip.copy_from_slice(&payload[14..18]);
    target_mac.copy_from_slice(&payload[18..24]);
    target_ip.copy_from_slice(&payload[24..28]);

    Ok(ArpPacket {
        htype,
        ptype,
        hlen,
        plen,
        opcode,
        sender_mac,
        sender_ip,
        target_mac,
        target_ip,
    })
}

pub fn parse_ipv4_header(payload: &[u8]) -> Result<(Ipv4Header, &[u8]), &'static str> {
    if payload.len() < 20 {
        return Err("IPv4 payload too short (<20B)");
    }
    let version = payload[0] >> 4;
    let ihl = payload[0] & 0x0F;
    if version != 4 {
        return Err("Not an IPv4 packet");
    }
    if ihl < 5 {
        return Err("IPv4 IHL too small (<5)");
    }
    let header_len = (ihl as usize) * 4;
    if payload.len() < header_len {
        return Err("IPv4 packet smaller than specified IHL");
    }

    let checksum_calc = compute_internet_checksum(&payload[0..header_len]);
    if checksum_calc != 0 {
        return Err("IPv4 header checksum failure");
    }

    let dscp_ecn = payload[1];
    let total_length = u16::from_be_bytes([payload[2], payload[3]]);
    let identification = u16::from_be_bytes([payload[4], payload[5]]);
    let flags_fragment_offset = u16::from_be_bytes([payload[6], payload[7]]);
    let ttl = payload[8];
    let protocol = payload[9];
    let header_checksum = u16::from_be_bytes([payload[10], payload[11]]);

    // Reject fragmented packets honestly
    if (flags_fragment_offset & 0x3FFF) != 0 {
        return Err("IPv4 fragmented packet dropped");
    }

    let mut src_ip = [0u8; 4];
    let mut dst_ip = [0u8; 4];
    src_ip.copy_from_slice(&payload[12..16]);
    dst_ip.copy_from_slice(&payload[16..20]);

    let ip_hdr = Ipv4Header {
        version,
        ihl,
        dscp_ecn,
        total_length,
        identification,
        flags_fragment_offset,
        ttl,
        protocol,
        header_checksum,
        src_ip,
        dst_ip,
    };

    let total_sz = total_length as usize;
    let available_total = payload.len();
    let body_end = if total_sz > header_len && total_sz <= available_total {
        total_sz
    } else {
        available_total
    };

    Ok((ip_hdr, &payload[header_len..body_end]))
}

pub fn parse_icmp_header(payload: &[u8]) -> Result<(IcmpHeader, &[u8]), &'static str> {
    if payload.len() < 8 {
        return Err("ICMP packet too short (<8B)");
    }
    let checksum_calc = compute_internet_checksum(payload);
    if checksum_calc != 0 {
        return Err("ICMP checksum failure");
    }
    let icmp_type = payload[0];
    let code = payload[1];
    let checksum = u16::from_be_bytes([payload[2], payload[3]]);
    let mut rest_of_header = [0u8; 4];
    rest_of_header.copy_from_slice(&payload[4..8]);

    Ok((
        IcmpHeader { icmp_type, code, checksum, rest_of_header },
        &payload[8..],
    ))
}

// -----------------------------------------------------------------------------
// PACKET BUILDERS
// -----------------------------------------------------------------------------

pub fn build_arp_reply(
    request: &ArpPacket,
    our_mac: [u8; 6],
    our_ip: [u8; 4],
    out: &mut [u8],
) -> Result<usize, &'static str> {
    if out.len() < 42 {
        return Err("Buffer too small for Ethernet+ARP reply");
    }
    // Ethernet Header
    out[0..6].copy_from_slice(&request.sender_mac);
    out[6..12].copy_from_slice(&our_mac);
    out[12..14].copy_from_slice(&ETHERTYPE_ARP.to_be_bytes());

    // ARP Payload
    out[14..16].copy_from_slice(&ARP_HTYPE_ETHERNET.to_be_bytes());
    out[16..18].copy_from_slice(&ARP_PTYPE_IPV4.to_be_bytes());
    out[18] = 6; // HLEN
    out[19] = 4; // PLEN
    out[20..22].copy_from_slice(&ARP_OP_REPLY.to_be_bytes());
    out[22..28].copy_from_slice(&our_mac);
    out[28..32].copy_from_slice(&our_ip);
    out[32..38].copy_from_slice(&request.sender_mac);
    out[38..42].copy_from_slice(&request.sender_ip);

    Ok(42)
}

pub fn build_icmp_echo_reply(
    eth_req: &EthernetHeader,
    ip_req: &Ipv4Header,
    icmp_req: &IcmpHeader,
    icmp_data: &[u8],
    our_mac: [u8; 6],
    our_ip: [u8; 4],
    out: &mut [u8],
) -> Result<usize, &'static str> {
    let total_len = 14 + 20 + 8 + icmp_data.len();
    if out.len() < total_len {
        return Err("Buffer too small for ICMP Echo Reply");
    }

    // 1. Ethernet Header
    out[0..6].copy_from_slice(&eth_req.src_mac);
    out[6..12].copy_from_slice(&our_mac);
    out[12..14].copy_from_slice(&ETHERTYPE_IPV4.to_be_bytes());

    // 2. IPv4 Header
    let ip_total_len = (20 + 8 + icmp_data.len()) as u16;
    out[14] = 0x45; // Version 4, IHL 5
    out[15] = 0x00; // DSCP / ECN
    out[16..18].copy_from_slice(&ip_total_len.to_be_bytes());
    out[18..20].copy_from_slice(&0u16.to_be_bytes()); // ID
    out[20..22].copy_from_slice(&0x4000u16.to_be_bytes()); // Flags DF, frag 0
    out[22] = 64; // TTL
    out[23] = IP_PROTO_ICMP;
    out[24..26].copy_from_slice(&[0, 0]); // Checksum placeholder
    out[26..30].copy_from_slice(&our_ip);
    out[30..34].copy_from_slice(&ip_req.src_ip);

    let ip_csum = compute_internet_checksum(&out[14..34]);
    out[24..26].copy_from_slice(&ip_csum.to_be_bytes());

    // 3. ICMP Echo Reply Header
    out[34] = ICMP_TYPE_ECHO_REPLY;
    out[35] = 0; // Code
    out[36..38].copy_from_slice(&[0, 0]); // Checksum placeholder
    out[38..42].copy_from_slice(&icmp_req.rest_of_header); // Identifier & Sequence

    // 4. ICMP Payload
    out[42..total_len].copy_from_slice(icmp_data);

    let icmp_csum = compute_internet_checksum(&out[34..total_len]);
    out[36..38].copy_from_slice(&icmp_csum.to_be_bytes());

    Ok(total_len)
}

// -----------------------------------------------------------------------------
// ARBITRARY DISPATCHER / NETSTACK RUNTIME
// -----------------------------------------------------------------------------

pub struct NetStackEngine {
    pub our_mac: [u8; 6],
    pub our_ip: [u8; 4],
}

impl NetStackEngine {
    pub const fn new(our_mac: [u8; 6], our_ip: [u8; 4]) -> Self {
        Self { our_mac, our_ip }
    }

    pub fn process_packet(
        &self,
        rx_buf: &[u8],
        tx_buf: &mut [u8],
    ) -> Result<Option<usize>, &'static str> {
        let (eth_hdr, eth_payload) = parse_ethernet_header(rx_buf)?;

        match eth_hdr.ethertype {
            ETHERTYPE_ARP => {
                let arp = parse_arp_packet(eth_payload)?;
                if arp.opcode == ARP_OP_REQUEST && arp.target_ip == self.our_ip {
                    let len = build_arp_reply(&arp, self.our_mac, self.our_ip, tx_buf)?;
                    return Ok(Some(len));
                }
                Ok(None)
            }
            ETHERTYPE_IPV4 => {
                let (ip_hdr, ip_payload) = parse_ipv4_header(eth_payload)?;
                if ip_hdr.dst_ip != self.our_ip && ip_hdr.dst_ip != [255, 255, 255, 255] {
                    return Ok(None);
                }
                if ip_hdr.protocol == IP_PROTO_ICMP {
                    let (icmp_hdr, icmp_data) = parse_icmp_header(ip_payload)?;
                    if icmp_hdr.icmp_type == ICMP_TYPE_ECHO_REQUEST {
                        let len = build_icmp_echo_reply(
                            &eth_hdr,
                            &ip_hdr,
                            &icmp_hdr,
                            icmp_data,
                            self.our_mac,
                            self.our_ip,
                            tx_buf,
                        )?;
                        return Ok(Some(len));
                    }
                }
                Ok(None)
            }
            _ => {
                // Drop unknown ethertypes safely
                Ok(None)
            }
        }
    }
}

// -----------------------------------------------------------------------------
// EXTENSIVE PROTOCOL PARSER TESTS (>= 15 TESTS)
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_01_checksum_simple() {
        let data = [0x45, 0x00, 0x00, 0x3C, 0x1C, 0x46, 0x40, 0x00, 0x40, 0x06, 0x00, 0x00, 0x0A, 0x00, 0x02, 0x0F, 0x0A, 0x00, 0x02, 0x02];
        let csum = compute_internet_checksum(&data);
        assert_ne!(csum, 0);
    }

    #[test]
    fn test_02_parse_ethernet_header_valid() {
        let mut frame = [0u8; 14];
        frame[0..6].copy_from_slice(&[0x08, 0x00, 0x27, 0x11, 0x22, 0x33]);
        frame[6..12].copy_from_slice(&[0x08, 0x00, 0x27, 0x44, 0x55, 0x66]);
        frame[12..14].copy_from_slice(&0x0800u16.to_be_bytes());

        let (hdr, payload) = parse_ethernet_header(&frame).unwrap();
        assert_eq!(hdr.ethertype, 0x0800);
        assert_eq!(payload.len(), 0);
    }

    #[test]
    fn test_03_parse_ethernet_too_short() {
        let frame = [0u8; 10];
        assert!(parse_ethernet_header(&frame).is_err());
    }

    #[test]
    fn test_04_parse_arp_request_valid() {
        let mut arp = [0u8; 28];
        arp[0..2].copy_from_slice(&1u16.to_be_bytes());
        arp[2..4].copy_from_slice(&0x0800u16.to_be_bytes());
        arp[4] = 6;
        arp[5] = 4;
        arp[6..8].copy_from_slice(&1u16.to_be_bytes()); // Request
        arp[8..14].copy_from_slice(&[0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
        arp[14..18].copy_from_slice(&[10, 0, 2, 2]);
        arp[24..28].copy_from_slice(&[10, 0, 2, 15]);

        let parsed = parse_arp_packet(&arp).unwrap();
        assert_eq!(parsed.opcode, 1);
        assert_eq!(parsed.sender_ip, [10, 0, 2, 2]);
        assert_eq!(parsed.target_ip, [10, 0, 2, 15]);
    }

    #[test]
    fn test_05_parse_arp_short_fails() {
        let arp = [0u8; 20];
        assert!(parse_arp_packet(&arp).is_err());
    }

    #[test]
    fn test_06_build_arp_reply() {
        let mut arp = [0u8; 28];
        arp[0..2].copy_from_slice(&1u16.to_be_bytes());
        arp[2..4].copy_from_slice(&0x0800u16.to_be_bytes());
        arp[4] = 6;
        arp[5] = 4;
        arp[6..8].copy_from_slice(&1u16.to_be_bytes());
        arp[8..14].copy_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        arp[14..18].copy_from_slice(&[10, 0, 2, 2]);
        arp[24..28].copy_from_slice(&[10, 0, 2, 15]);
        let parsed = parse_arp_packet(&arp).unwrap();

        let mut out = [0u8; 64];
        let len = build_arp_reply(&parsed, [0x08, 0x00, 0x27, 0x5E, 0x08, 0x33], [10, 0, 2, 15], &mut out).unwrap();
        assert_eq!(len, 42);
        assert_eq!(out[12..14], [0x08, 0x06]); // ETHERTYPE_ARP
        assert_eq!(out[20..22], [0x00, 0x02]); // ARP_REPLY
    }

    #[test]
    fn test_07_parse_ipv4_valid() {
        let mut pkt = [0u8; 20];
        pkt[0] = 0x45;
        pkt[2..4].copy_from_slice(&20u16.to_be_bytes());
        pkt[8] = 64;
        pkt[9] = 1; // ICMP
        pkt[12..16].copy_from_slice(&[10, 0, 2, 2]);
        pkt[16..20].copy_from_slice(&[10, 0, 2, 15]);

        let csum = compute_internet_checksum(&pkt);
        pkt[10..12].copy_from_slice(&csum.to_be_bytes());

        let (hdr, payload) = parse_ipv4_header(&pkt).unwrap();
        assert_eq!(hdr.version, 4);
        assert_eq!(hdr.protocol, 1);
        assert_eq!(payload.len(), 0);
    }

    #[test]
    fn test_08_parse_ipv4_checksum_corrupted() {
        let mut pkt = [0u8; 20];
        pkt[0] = 0x45;
        pkt[2..4].copy_from_slice(&20u16.to_be_bytes());
        pkt[10..12].copy_from_slice(&0xDEADu16.to_be_bytes());
        assert!(parse_ipv4_header(&pkt).is_err());
    }

    #[test]
    fn test_09_parse_ipv4_fragment_dropped() {
        let mut pkt = [0u8; 20];
        pkt[0] = 0x45;
        pkt[2..4].copy_from_slice(&20u16.to_be_bytes());
        pkt[6..8].copy_from_slice(&0x2000u16.to_be_bytes()); // MF flag set
        let csum = compute_internet_checksum(&pkt);
        pkt[10..12].copy_from_slice(&csum.to_be_bytes());
        assert!(parse_ipv4_header(&pkt).is_err());
    }

    #[test]
    fn test_10_parse_icmp_echo_request_valid() {
        let mut icmp = [0u8; 12];
        icmp[0] = 8; // Echo request
        icmp[1] = 0; // Code 0
        icmp[4..6].copy_from_slice(&0x1234u16.to_be_bytes()); // ID
        icmp[6..8].copy_from_slice(&0x0001u16.to_be_bytes()); // Seq
        icmp[8..12].copy_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]); // Data
        let csum = compute_internet_checksum(&icmp);
        icmp[2..4].copy_from_slice(&csum.to_be_bytes());

        let (hdr, payload) = parse_icmp_header(&icmp).unwrap();
        assert_eq!(hdr.icmp_type, 8);
        assert_eq!(payload, &[0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn test_11_parse_icmp_bad_checksum() {
        let mut icmp = [0u8; 8];
        icmp[0] = 8;
        icmp[2..4].copy_from_slice(&0x1111u16.to_be_bytes());
        assert!(parse_icmp_header(&icmp).is_err());
    }

    #[test]
    fn test_12_build_icmp_echo_reply() {
        let eth_req = EthernetHeader {
            dst_mac: [0x08, 0x00, 0x27, 0x5E, 0x08, 0x33],
            src_mac: [0x00, 0x11, 0x22, 0x33, 0x44, 0x55],
            ethertype: 0x0800,
        };
        let ip_req = Ipv4Header {
            version: 4,
            ihl: 5,
            dscp_ecn: 0,
            total_length: 28,
            identification: 1,
            flags_fragment_offset: 0,
            ttl: 64,
            protocol: 1,
            header_checksum: 0,
            src_ip: [10, 0, 2, 2],
            dst_ip: [10, 0, 2, 15],
        };
        let icmp_req = IcmpHeader {
            icmp_type: 8,
            code: 0,
            checksum: 0,
            rest_of_header: [0x12, 0x34, 0x00, 0x01],
        };
        let icmp_data = [0xAA, 0xBB, 0xCC];

        let mut out = [0u8; 128];
        let len = build_icmp_echo_reply(
            &eth_req,
            &ip_req,
            &icmp_req,
            &icmp_data,
            [0x08, 0x00, 0x27, 0x5E, 0x08, 0x33],
            [10, 0, 2, 15],
            &mut out,
        ).unwrap();

        assert_eq!(len, 14 + 20 + 8 + 3);
        assert_eq!(out[34], 0); // Echo reply type
    }

    #[test]
    fn test_13_netstack_engine_arp_cycle() {
        let engine = NetStackEngine::new([0x08, 0x00, 0x27, 0x5E, 0x08, 0x33], [10, 0, 2, 15]);

        let mut arp_frame = [0u8; 42];
        arp_frame[0..6].copy_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
        arp_frame[6..12].copy_from_slice(&[0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
        arp_frame[12..14].copy_from_slice(&0x0806u16.to_be_bytes());

        arp_frame[14..16].copy_from_slice(&1u16.to_be_bytes());
        arp_frame[16..18].copy_from_slice(&0x0800u16.to_be_bytes());
        arp_frame[18] = 6;
        arp_frame[19] = 4;
        arp_frame[20..22].copy_from_slice(&1u16.to_be_bytes()); // Request
        arp_frame[22..28].copy_from_slice(&[0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
        arp_frame[28..32].copy_from_slice(&[10, 0, 2, 2]);
        arp_frame[38..42].copy_from_slice(&[10, 0, 2, 15]);

        let mut tx_buf = [0u8; 128];
        let resp = engine.process_packet(&arp_frame, &mut tx_buf).unwrap();
        assert!(resp.is_some());
        assert_eq!(resp.unwrap(), 42);
    }

    #[test]
    fn test_14_netstack_engine_ignore_foreign_ip() {
        let engine = NetStackEngine::new([0x08, 0x00, 0x27, 0x5E, 0x08, 0x33], [10, 0, 2, 15]);

        let mut pkt = [0u8; 34];
        pkt[0..6].copy_from_slice(&[0x08, 0x00, 0x27, 0x5E, 0x08, 0x33]);
        pkt[6..12].copy_from_slice(&[0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
        pkt[12..14].copy_from_slice(&0x0800u16.to_be_bytes());

        pkt[14] = 0x45;
        pkt[16..18].copy_from_slice(&20u16.to_be_bytes());
        pkt[22] = 64;
        pkt[23] = 1;
        pkt[26..30].copy_from_slice(&[10, 0, 2, 2]);
        pkt[30..34].copy_from_slice(&[10, 0, 2, 99]); // Different IP!
        let csum = compute_internet_checksum(&pkt[14..34]);
        pkt[24..26].copy_from_slice(&csum.to_be_bytes());

        let mut tx_buf = [0u8; 128];
        let resp = engine.process_packet(&pkt, &mut tx_buf).unwrap();
        assert!(resp.is_none());
    }

    #[test]
    fn test_15_netstack_engine_unknown_ethertype_ignored() {
        let engine = NetStackEngine::new([0x08, 0x00, 0x27, 0x5E, 0x08, 0x33], [10, 0, 2, 15]);
        let mut pkt = [0u8; 20];
        pkt[12..14].copy_from_slice(&0x86DDu16.to_be_bytes()); // IPv6

        let mut tx_buf = [0u8; 128];
        let resp = engine.process_packet(&pkt, &mut tx_buf).unwrap();
        assert!(resp.is_none());
    }
}
