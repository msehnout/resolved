extern crate env_logger;
#[macro_use] extern crate lazy_static;
extern crate libresolv;
#[macro_use] extern crate log;
extern crate regex;

use libresolv::message::Message;
use libresolv::wire::{FromWire, ToWire};
use regex::Regex;

use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::str::FromStr;

const RESOLV_CONF: &'static str = "/etc/resolv.conf";

lazy_static! {
    static ref NAMESERVER_RE: Regex = Regex::new(r"^nameserver (?P<addr>\d+.\d+.\d+.\d+)$")
                                            .expect("The regex to match nameservers is broken.");
}

#[cfg(target_os="linux")]
fn get_resolvers() -> io::Result<Vec<Ipv4Addr>> {
    // Load file
    let mut f = File::open(RESOLV_CONF)?;
    let mut s = String::new();
    f.read_to_string(&mut s)?;
    // Parse only nameservers
    let ret = s.lines()
               .filter_map(|line| {
                   NAMESERVER_RE.captures(line)
               })
               .filter_map(|cap_addr| {
                    Ipv4Addr::from_str(&cap_addr["addr"]).ok()
               })
               .collect();
    Ok(ret)
}

fn glue_response(id: u16, rest: &[u8]) -> Vec<u8> {
    let mut resp_buffer = Vec::new();
    resp_buffer.extend(id.to_wire());
    resp_buffer.extend_from_slice(rest);
    resp_buffer
}

fn main() {
    env_logger::init().expect("Logging functionality could not be initialized.");
    let resolvers = get_resolvers().expect("No local resolvers were found.");
    if resolvers.len() == 0 {
        error!("There is no resolver in the config file!");
        return;
    }
    debug!("Found these resolvers {:?}", resolvers);
    let zero_socket_addr = SocketAddrV4::new(Ipv4Addr::from([127, 0, 0, 1]), 0);
    let listen_socket = UdpSocket::bind(zero_socket_addr).unwrap();
    println!("Listening on: {:?}", listen_socket);
    let mut cache: HashMap<String, Vec<u8>> = HashMap::new();
    loop {
        let mut buffer = [0u8; 4096];
        let (len, source) = listen_socket.recv_from(&mut buffer).unwrap();
        if let Ok((_, msg)) = Message::from_wire(&buffer) {
            if msg.is_query() {
                let q = &msg.question[0].name;
                println!("Got query for this name: {}", msg.question[0].name);
                if let Some(resp) = cache.get(q) {
                    println!("Cache hit!");
                    let id = msg.header.id;
                    listen_socket.send_to(&glue_response(id, resp), source).expect("6");
                    continue;
                } else {
                    println!("Cache miss!");
                }
                let query_socket = UdpSocket::bind(zero_socket_addr).expect("1");
                let resolver_addr: Ipv4Addr = Ipv4Addr::from([127, 0, 0, 1]);
                query_socket.connect(SocketAddrV4::new(resolver_addr, 53)).expect("4");
                query_socket.send(&mut buffer[..len]).expect("2");
                let mut query_buffer = [0u8; 4096];
                let len = query_socket.recv(&mut query_buffer).expect("3");
                if let Ok((_, resp)) = Message::from_wire(&query_buffer) {
                    println!("Answer is: {:?}", resp.answer);
                    cache.insert(msg.question[0].name.to_owned(), query_buffer[2..len].to_owned());
                    let id = msg.header.id;
                    listen_socket.send_to(&glue_response(id, &query_buffer[2..len]), source).expect("5");
                }
            }
        }
    }
}
