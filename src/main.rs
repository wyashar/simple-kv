use std::io::{BufRead, BufReader};
use std::net::{TcpListener, TcpStream};

fn main() -> std::io::Result<()> {
    let tcp_listener = match TcpListener::bind("127.0.0.1:8080") {
        Ok(listener) => {
            println!("Connection established on port {}", listener.local_addr()?.port());
            listener
        },
        Err(error) => panic!("Connection refused: {:?}", error)
    };

    tcp_listener
        .incoming()
        .for_each( | peer: Result<TcpStream, std::io::Error> | {
            match peer {
                Ok(stream) => handle_connection(&stream),
                Err(stream) => eprintln!("Failed to establish a connection: {:?}", stream),
            }
        });


    Ok(())
}

fn handle_connection(stream: &TcpStream) -> () {
    println!("Attempting to connect to peer...");

    let peer_addr = match stream.peer_addr() {
        Ok(addr) => addr,
        Err(e) => {
            eprintln!("Failed to get peer address: {}", e);
            return;
        }
    };

    println!("Connection established to: {}", peer_addr);


    for line in BufReader::new(stream).lines() {
        match line {
            Ok(line) => println!("{}", line),
            Err(e) => {
                eprintln!("Client {} disconnected with error: {:?}", peer_addr, e);
                return;
            },
        }
    }

    println!("Client {} disconnected without error", peer_addr);
}
