use anyhow::Result;
use std::{
    net::{Ipv4Addr, UdpSocket},
    str,
    time::{Duration, Instant},
};

const SSDP_MULTICAST_ADDR: &str = "239.255.255.250";
const SSDP_MULTICAST_PORT: &str = "1900";

/// Parses an SSDP response string to extract the device name and IP address.
///
/// Returns `Some((name, ip))` if both are found in the headers, `None` otherwise.
fn parse_name_and_ip(s: &str) -> Option<(String, String)> {
    let headers = s.split("\r\n");
    let (mut name, mut ip) = (None, None);
    for header in headers {
        if header.starts_with("Location") {
            let mut split = header.split(' ');
            ip = Some(
                split
                    .next_back()
                    .unwrap()
                    .strip_prefix("http://")
                    .unwrap()
                    .split(':')
                    .next()
                    .unwrap()
                    .to_string(),
            );
        } else if header.starts_with("nl-devicename") {
            let mut split = header.split(':');
            name = Some(split.next_back().unwrap().trim_start().to_string());
        }
    }
    if let (Some(name), Some(ip)) = (name, ip) {
        Some((name, ip))
    } else {
        None
    }
}

/// Discovers Nanoleaf devices on the local network using the SSDP M-SEARCH protocol.
///
/// Sends multicast search requests for known Nanoleaf device types including Canvas (nl29),
/// Shapes (nl42), Elements (nl52), and Aurora Light Panels.
///
/// Listens for responses for a timeout of 10 seconds, avoiding duplicate devices based on IP address.
///
/// # Returns
///
/// A tuple `(Vec<String>, Vec<Ipv4Addr>)` containing the discovered device names and their corresponding IP addresses.
///
/// # Errors
///
/// Returns an `anyhow::Error` if socket binding, multicast joining, sending requests,
/// receiving responses, or IP parsing fails.
pub fn ssdp_msearch() -> Result<(Vec<String>, Vec<Ipv4Addr>)> {
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.join_multicast_v4(&SSDP_MULTICAST_ADDR.parse()?, &"0.0.0.0".parse()?)?;

    // Search for different Nanoleaf device types
    // nl29 = Canvas, nl42 = Shapes, nl52 = Elements
    let device_types = [
        "nanoleaf:nl29",
        "nanoleaf:nl42",
        "nanoleaf:nl52",
        "nanoleaf_aurora:light",
    ];
    for device_type in &device_types {
        socket.send_to(
            format!(
                "M-SEARCH * HTTP/1.1\r\nHOST: {SSDP_MULTICAST_ADDR}:{SSDP_MULTICAST_PORT}\r\n\
                MAN: \"ssdp:discover\"\r\nMX: 1\r\nST: {}\r\n\r\n",
                device_type
            )
            .as_bytes(),
            format!("{SSDP_MULTICAST_ADDR}:{SSDP_MULTICAST_PORT}"),
        )?;
    }

    socket.set_read_timeout(Some(Duration::from_secs(1)))?;
    let (mut ips, mut names) = (vec![], vec![]);
    let mut buf = [0; 1 << 10];
    let timeout = Duration::from_secs(10);
    println!(
        "Listening for Nanoleaf devices (Canvas/Shapes/Elements/Light Panels), timing out in {} seconds",
        timeout.as_secs()
    );
    let timer = Instant::now();
    loop {
        if let Ok((size, _)) = socket.recv_from(&mut buf) {
            let response = str::from_utf8(&buf[..size]).unwrap();
            if let Some((name, ip)) = parse_name_and_ip(response) {
                // Avoid adding duplicate devices
                let parsed_ip = ip.parse::<Ipv4Addr>()?;
                if !ips.contains(&parsed_ip) {
                    println!("Discovered device `{}` with IP address {}", name, ip);
                    names.push(name);
                    ips.push(parsed_ip);
                }
            }
        }
        if timer.elapsed() >= timeout {
            break;
        }
    }

    Ok((names, ips))
}
