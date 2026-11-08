//! Waking the other computer with a Wake-on-LAN "magic packet".
//!
//! Each side tells the other the hardware address of the network card the
//! connection uses (in its [`sc_proto::Hello`]); the window keeps it, so the
//! other computer can be woken while it sleeps (or, a PC set up for it in
//! its firmware, while it is off). A sleeping Mac wakes this way if "Wake
//! for network access" is on; a Mac that is shut down cannot.

use std::collections::BTreeSet;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};

/// Port the packet is sent to; network cards look at any port.
const PORT: u16 = 9;
/// Sent a few times: it is a single UDP datagram that may get lost.
const REPEAT: usize = 3;

pub type Mac = [u8; 6];

/// Six 0xFF bytes, then the address 16 times.
pub fn magic_packet(mac: Mac) -> Vec<u8> {
    let mut packet = vec![0xFF; 6];
    for _ in 0..16 {
        packet.extend_from_slice(&mac);
    }
    packet
}

/// `AA:BB:CC:DD:EE:FF` or `aa-bb-cc-dd-ee-ff`.
pub fn parse_mac(text: &str) -> Option<Mac> {
    let mut mac = [0u8; 6];
    let mut parts = text.trim().split([':', '-']);
    for byte in &mut mac {
        let part = parts.next()?;
        if part.len() != 2 {
            return None;
        }
        *byte = u8::from_str_radix(part, 16).ok()?;
    }
    parts.next().is_none().then_some(mac)
}

pub fn format_mac(mac: Mac) -> String {
    mac.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":")
}

/// The hardware address of the network card that has this address.
pub fn mac_for(ip: IpAddr) -> Option<Mac> {
    netdev::get_interfaces()
        .into_iter()
        .filter(|card| !card.is_loopback())
        .find(|card| match ip {
            IpAddr::V4(ip) => card.ipv4.iter().any(|net| net.addr() == ip),
            IpAddr::V6(ip) => card.ipv6.iter().any(|net| net.addr() == ip),
        })
        .and_then(|card| card.mac_addr)
        .map(|mac| mac.octets())
        .filter(|mac| *mac != [0; 6])
}

/// Broadcasts the packet on every local network.
pub fn send(mac: Mac) -> io::Result<()> {
    let mut targets = BTreeSet::from([Ipv4Addr::BROADCAST]);
    for card in netdev::get_interfaces() {
        if card.is_up() && !card.is_loopback() {
            targets.extend(card.ipv4.iter().map(|net| net.broadcast()));
        }
    }
    let targets: Vec<SocketAddr> =
        targets.into_iter().map(|ip| SocketAddrV4::new(ip, PORT).into()).collect();
    send_to(mac, &targets)
}

/// Succeeds if the packet left for at least one target.
fn send_to(mac: Mac, targets: &[SocketAddr]) -> io::Result<()> {
    let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))?;
    socket.set_broadcast(true)?;
    let packet = magic_packet(mac);
    let mut result = Err(io::Error::other("no network to send on"));
    for _ in 0..REPEAT {
        for target in targets {
            match socket.send_to(&packet, target) {
                Ok(_) => result = Ok(()),
                Err(e) if result.is_err() => result = Err(e),
                Err(_) => {}
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    const MAC: Mac = [0x3C, 0x22, 0xFB, 0x0A, 0xB1, 0x07];

    #[test]
    fn magic_packet_is_ff_then_the_address_16_times() {
        let packet = magic_packet(MAC);
        assert_eq!(packet.len(), 102);
        assert_eq!(packet[..6], [0xFF; 6]);
        assert!(packet[6..].chunks(6).all(|chunk| chunk == MAC));
    }

    #[test]
    fn addresses_parse_and_format() {
        assert_eq!(parse_mac("3c:22:fb:0a:b1:07"), Some(MAC));
        assert_eq!(parse_mac(" 3C-22-FB-0A-B1-07 "), Some(MAC));
        assert_eq!(format_mac(MAC), "3C:22:FB:0A:B1:07");
        for bad in
            ["", "3c:22:fb:0a:b1", "3c:22:fb:0a:b1:07:00", "3c:22:fb:0a:b1:zz", "3c22fb0ab107"]
        {
            assert_eq!(parse_mac(bad), None, "{bad}");
        }
    }

    #[test]
    fn packet_arrives() {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        send_to(MAC, &[socket.local_addr().unwrap()]).unwrap();
        let mut buf = [0u8; 200];
        let len = socket.recv(&mut buf).unwrap();
        assert_eq!(buf[..len], magic_packet(MAC)[..]);
    }
}
