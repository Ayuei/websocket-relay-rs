// #![deny(warnings)]

use tokio::sync::broadcast;
use warp::ws::{WebSocket, Message};
use warp::Filter;
use futures::SinkExt;

use futures_util::{StreamExt};
use tokio_tungstenite::{connect_async};
use tokio::sync::broadcast::{Sender, Receiver};
use std::{env, process};

async fn run_ws(send: Sender<String>, port: String) {
    let url = format!("ws://localhost:{}/Messages", port);

    // Connect to parent websocket asynchronously
    let (ws_stream, _) = connect_async(url)
        .await
        .expect("Failed to connect");

    println!("Connected to Parent WebSocket");

    let (_write, mut read) = ws_stream.split();

    // While there are message with OPCODE::TEXT, parse and forward to all connected clients via
    // Broadcast Sender.
    while let Some(msg) = read.next().await {
        match msg {
            Ok(m) => {
                let m = m.to_owned().to_string();
                println!("Received from parent socket: {}", m);
                send.send(m).unwrap();
            },
            Err(e) => {
                // Parent socket hung up, abort.
                eprintln!("Error {}", e);
                process::exit(0x0100);
            }
        }
    };
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();

    let port = match args.len() {
        2 => {
            match &args[1].parse::<i32>() {
                Ok(_) => {
                    args[1].clone()
                },
                Err(_) => {
                    eprintln!("Error: port is not a number");
                    return;
                }
            }
        },
        _ => {
            println!("USAGE: websocket-relay-server [listening on server port]");
            return;
        }
    };

    // Set up a broadcast channel for use with parent socket
    let (send, _receiver) = broadcast::channel::<String>(999);
    // Set up a clone to be passed to all clients
    let send2 = send.clone();

    // Each client will get a receiver that is subscribed to the main channel
    let rx = warp::any().map(move || send2.subscribe());

    let relay= warp::path("relay")
        // The `ws()` filter will prepare Websocket handshake...
        .and(warp::ws())
        .and(rx)
        .map(move |ws: warp::ws::Ws, rx| {
            // This will call our function if the handshake succeeds.
            ws.on_upgrade(move |socket| client_connection(socket, rx))
        });

    // Start the event loop
    tokio::join!(
        run_ws(send, port),
        warp::serve(relay).run(([127, 0, 0, 1], 3030))
    );
}

async fn client_connection(ws: WebSocket, mut rcv: Receiver<String>) {
    eprintln!("New connection");
    let (mut tx, _rx) = ws.split();

    loop {
        // Once something is received from parent socket, forward to the client
        match rcv.recv().await {
            Ok(m) => {
                // If client is still connected, this should work.
                if let Err(e) = tx.send(Message::text(m)).await {
                    // Client has disconnected, exit.
                    eprintln!("Child socket: {}", e);
                    return
                }
            },
            // Parent socket has disconnected, abort.
            Err(e) => {
                eprintln!("Parent socket: {}", e);
                return
            }
        }
    }
}