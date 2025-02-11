use anyhow::Result;
use std::{
    net::{Ipv4Addr, UdpSocket},
    str,
    time::{Duration, Instant},
};

const SSDP_MULTICAST_ADDR: &str = "239.255.255.250";
const SSDP_MULTICAST_PORT: &str = "1900";

fn parse_name_and_ip(s: &str) -> Option<(String, String)> {
    let headers = s.split("\r\n");
    let (mut name, mut ip) = (None, None);
    for header in headers {
        if header.starts_with("Location") {
            let split = header.split(' ');
            ip = Some(
                split
                    .last()
                    .unwrap()
                    .strip_prefix("http://")
                    .unwrap()
                    .split(':')
                    .next()
                    .unwrap()
                    .to_string(),
            );
        } else if header.starts_with("nl-devicename") {
            let split = header.split(':');
            name = Some(split.last().unwrap().trim_start().to_string());
        }
    }
    if let (Some(name), Some(ip)) = (name, ip) {
        Some((name, ip))
    } else {
        None
    }
}

pub fn ssdp_msearch() -> Result<(Vec<String>, Vec<Ipv4Addr>)> {
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.join_multicast_v4(&SSDP_MULTICAST_ADDR.parse()?, &"0.0.0.0".parse()?)?;
    socket.send_to(
        format!(
            "M-SEARCH * HTTP/1.1\r\nHOST: {SSDP_MULTICAST_ADDR}:{SSDP_MULTICAST_PORT}\r\n\
            MAN: \"ssdp:discover\"\r\nMX: 1\r\nST: nanoleaf:nl29\r\n\r\n"
        )
        .as_bytes(),
        format!("{SSDP_MULTICAST_ADDR}:{SSDP_MULTICAST_PORT}"),
    )?;
    socket.set_read_timeout(Some(Duration::from_secs(1)))?;
    let (mut ips, mut names) = (vec![], vec![]);
    let mut buf = [0; 1 << 10];
    let timeout = Duration::from_secs(10);
    println!(
        "Listening for Nanoleaf devices, timing out in {} seconds",
        timeout.as_secs()
    );
    let timer = Instant::now();
    loop {
        if let Ok((size, _)) = socket.recv_from(&mut buf) {
            let response = str::from_utf8(&buf[..size]).unwrap();
            if let Some((name, ip)) = parse_name_and_ip(response) {
                println!("Discovered device `{}` with IP address {}", name, ip);
                names.push(name);
                let ip = ip.parse::<Ipv4Addr>()?;
                ips.push(ip);
            }
        }
        if timer.elapsed() >= timeout {
            break;
        }
    }

    Ok((names, ips))
}
