#![feature(let_chains)]
use kinode_process_lib::{
    await_message, call_init, get_blob, get_typed_state, http, println, set_state, Address,
    LazyLoadBlob, Message, NodeId, Request, Response,
};

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use rand::prelude::*;
use rand_chacha::ChaCha20Rng;
use chrono::Utc;

extern crate rand;
extern crate rand_chacha;
extern crate chrono;


const ICON: &str = include_str!("icon");

//
// Our "rng protocol" request/response format. We'll always serialize these
// to a byte vector and send them over IPC.
//

#[derive(Debug, Serialize, Deserialize)]
enum RngRequest {
    NewRandom { target: String },
    // History,
}
// kit i kinode_rng:kinode_rng:magiccity.os '{"NewRandom": { "target": "fake.os"}}'
#[derive(Debug, Serialize, Deserialize)]
enum RngResponse {
    Random { result: Random },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Random {
    kinode: String,
    value: usize,
    timestamp: chrono::DateTime<chrono::Utc>
}



#[derive(Debug, Serialize, Deserialize)]
struct RngState {
    pub randoms: Vec<Random>, 
    pub clients: HashSet<u32>,       
}

#[derive(Debug, Serialize, Deserialize)]
struct Randoms {
    pub randoms: Vec<Random>
}

fn save_rng_state(state: &RngState) {
    set_state(&bincode::serialize(&state.randoms).unwrap());
}

fn load_rng_state() -> RngState {
    match get_typed_state(|bytes| Ok(bincode::deserialize::<Vec<Random>>(bytes)?)) {
        Some(randoms) => RngState {
            randoms,
            clients: HashSet::new(),
        },
        None => RngState {
            randoms: Vec::new(),
            clients: HashSet::new(),
        },
    }
}

fn send_ws_update(our: &Address, random: &Random, open_channels: &HashSet<u32>) -> anyhow::Result<()> {
    for channel in open_channels {
        Request::new()
            .target((&our.node, "http_server", "distro", "sys"))
            .body(serde_json::to_vec(
                &http::HttpServerAction::WebSocketPush {
                    channel_id: *channel,
                    message_type: http::WsMessageType::Binary,
                },
            )?)
            .blob(LazyLoadBlob {
                mime: Some("application/json".to_string()),
                bytes: serde_json::json!({
                    "kind": "rng",
                    "data": random,
                })
                .to_string()
                .into_bytes(),
            })
            .send()?;
    }
    Ok(())
}

// Boilerplate: generate the wasm bindings for a process
wit_bindgen::generate!({
    path: "wit",
    world: "process",
    exports: {
        world: Component,
    },
});
// After generating bindings, use this macro to define the Component struct
// and its init() function, which the kernel will look for on startup.
call_init!(initialize);

fn initialize(our: Address) {
    // A little printout to show in terminal that the process has started.
    println!("started");

    // add ourselves to the homepage
    Request::to(("our", "homepage", "homepage", "sys"))
        .body(
            serde_json::json!({
                "Add": {
                    "label": "Rng",
                    "icon": ICON,
                    "path": "/", // just our root
                }
            })
            .to_string()
            .as_bytes()
            .to_vec(),
        )
        .send()
        .unwrap();

    // Serve the index.html and other UI files found in pkg/ui at the root path.
    // authenticated=true, local_only=false
    http::serve_ui(&our, "ui", true, false, vec!["/"]).unwrap();

    // Allow HTTP requests to be made to /randoms; they will be handled dynamically.
    http::bind_http_path("/randoms", true, false).unwrap();

    // Allow websockets to be opened at / (our process ID will be prepended).
    http::bind_ws_path("/", true, false).unwrap();

    // Grab our state, then enter the main event loop.
    let mut state: RngState = load_rng_state();
    main_loop(&our, &mut state);
}

fn main_loop(our: &Address, state: &mut RngState) {
    loop {
        // Call await_message() to wait for any incoming messages.
        // If we get a network error, make a print and throw it away.
        // In a high-quality consumer-grade app, we'd want to explicitly handle
        // this and surface it to the user.
        match await_message() {
            Err(send_error) => {
                println!("got network error: {send_error:?}");
                continue;
            }
            Ok(message) => match handle_request(&our, &message, state) {
                Ok(()) => continue,
                Err(e) => println!("error handling request: {:?}", e),
            },
        }
    }
}

/// Handle rng protocol messages from ourself *or* other nodes.
fn handle_request(our: &Address, message: &Message, state: &mut RngState) -> anyhow::Result<()> {
    // Throw away responses. We never expect any responses *here*, because for every
    // rng protocol request, we *await* its response in-place. This is appropriate
    // for direct node<>node comms, less appropriate for other circumstances...
    println!("here again ");
    if !message.is_request() {
        return Ok(());
    }
    // If the request is from another node, handle it as an incoming request.
    // Note that we can enforce the ProcessId as well, but it shouldn't be a trusted
    // piece of information, since another node can easily spoof any ProcessId on a request.
    // It can still be useful simply as a protocol-level switch to handle different kinds of
    // requests from the same node, with the knowledge that the remote node can finagle with
    // which ProcessId a given message can be from. It's their code, after all.
    if message.source().node != our.node {
        // Deserialize the request IPC to our format, and throw it away if it
        // doesn't fit.
        let Ok(rng_request) = serde_json::from_slice::<RngRequest>(message.body()) else {
            return Err(anyhow::anyhow!("invalid rng request"));
        };
        handle_rng_request(our, &message.source().node, state, &rng_request)
    // ...and if the request is from ourselves, handle it as our own!
    // Note that since this is a local request, we *can* trust the ProcessId.
    // Here, we'll accept messages from the local terminal so as to make this a "CLI" app.
    } else if message.source().node == our.node
        && message.source().process == "terminal:terminal:sys"
    {
        let Ok(rng_request) = serde_json::from_slice::<RngRequest>(message.body()) else {
            return Err(anyhow::anyhow!("invalid rng request"));
        };
        handle_local_request(state, &rng_request)
    } else if message.source().node == our.node
        && message.source().process == "http_server:distro:sys"
    {
        
        // receive HTTP requests and websocket connection messages from our server
        match serde_json::from_slice::<http::HttpServerRequest>(message.body())? {
            http::HttpServerRequest::Http(ref incoming) => {
                let blob_json = serde_json::from_slice::<serde_json::Value>(&message.body())?;
                let Some(target_kinode) = blob_json["kinode"].as_str() else {
                return Ok(http::send_response(
                    http::StatusCode::BAD_REQUEST,
                    None,
                    vec![],
                ));
            };
                println!("before rng");  
                let rng_request = RngRequest::NewRandom { target: target_kinode.to_string() };
                if target_kinode == our.node {
                    println!("our http");

                    handle_local_request(state, &rng_request)} else {
                        println!("other http");
                    match handle_http_request(our, state, incoming) {
                        Ok(()) => Ok(()),
                        Err(e) => {
                            http::send_response(
                                http::StatusCode::SERVICE_UNAVAILABLE,
                                None,
                                "Service Unavailable".to_string().as_bytes().to_vec(),
                            );
                            Err(anyhow::anyhow!("error handling http request: {e:?}"))
                        }
                    }
                }
            }
            http::HttpServerRequest::WebSocketOpen { channel_id, .. } => {
                // We know this is authenticated and unencrypted because we only
                // bound one path, the root path. So we know that client
                // frontend opened a websocket and can send updates
                state.clients.insert(channel_id);
                Ok(())
            }
            http::HttpServerRequest::WebSocketClose(channel_id) => {
                // client frontend closed a websocket
                state.clients.remove(&channel_id);
                Ok(())
            }
            http::HttpServerRequest::WebSocketPush { .. } => {
                // client frontend sent a websocket message
                // we don't expect this! we only use websockets to push updates
                Ok(())
            }
        }
    } else {
        // If we get a request from ourselves that isn't from the terminal, we'll just
        // throw it away. This is a good place to put a printout to show that we've
        // received a request from ourselves that we don't know how to handle.
        return Err(anyhow::anyhow!(
            "got request from not-the-terminal, ignoring"
        ));
    }
}

/// Handle rng protocol messages from other nodes.
fn handle_rng_request(
    our: &Address,
    source_node: &NodeId,
    state: &mut RngState,
    action: &RngRequest,
) -> anyhow::Result<()> {
    println!("handling action from {source_node}: {action:?}");

    // let kinode = source_node;

    match action {
        RngRequest::NewRandom {target }=> {
            let mut rng = ChaCha20Rng::from_entropy();
            let random = Random {
                kinode: target.to_string(),
                value: rng.gen_range(0..2),
                timestamp: Utc::now()
            };
            // Use our helper function to persist state after every action.
            // The simplest and most trivial way to keep state. You'll want to
            // use a database or something in a real app, and consider performance
            // when doing intensive data-based operations.
            send_ws_update(&our, &random, &state.clients)?;
            //  save other below? 
            // state.randoms.insert(random);
            // save_rng_state(&state);
            Response::new()
                .body(serde_json::to_vec(&RngResponse::Random{result: random.clone()})?)
                .send()
        }
       
    }
}

/// Handle actions we are performing. Here's where we'll send_and_await various requests.
fn handle_local_request(
    state: &mut RngState,
    action: &RngRequest,
) -> anyhow::Result<()> {
    match action {
        RngRequest::NewRandom { target } => {
            println!("here local");
            let mut rng = ChaCha20Rng::from_entropy();
            let random = Random {
                kinode: target.to_string(),
                value: rng.gen_range(0..2),
                timestamp: Utc::now()
            };
            state.randoms.push(random.clone());
            println!("value: {}",random.value);
            save_rng_state(&state);
            Ok(())
        }
    }
}

/// Handle HTTP requests from our own frontend.
fn handle_http_request(
    our: &Address,
    state: &mut RngState,
    http_request: &http::IncomingHttpRequest,
) -> anyhow::Result<()> {
    if http_request.bound_path(Some(&our.process.to_string())) != "/randoms" {
        http::send_response(
            http::StatusCode::NOT_FOUND,
            None,
            "Not Found".to_string().as_bytes().to_vec(),
        );
        return Ok(());
    }
    match http_request.method()?.as_str() {
        // on GET: get all randoms
        "GET" => Ok(http::send_response(
            http::StatusCode::OK,
            Some(HashMap::from([(
                String::from("Content-Type"),
                String::from("application/json"),
            )])),
            serde_json::to_vec(&state.randoms)?,
        )),
        // on POST: get a new random from somone
        "POST" => {
            let Some(blob) = get_blob() else {
                return Ok(http::send_response(
                    http::StatusCode::BAD_REQUEST,
                    None,
                    vec![],
                ));
            };
            let blob_json = serde_json::from_slice::<serde_json::Value>(&blob.bytes)?;
            let Some(target_kinode) = blob_json["kinode"].as_str() else {
                return Ok(http::send_response(
                    http::StatusCode::BAD_REQUEST,
                    None,
                    vec![],
                ));
            };
                //here
                println!("here http");
           
            let Ok(msg) = Request::new()
                .target((target_kinode, our.process.clone()))
                .body(serde_json::to_vec(&RngRequest::NewRandom {target: target_kinode.to_string()})?)
                .send_and_await_response(5)?
            else {
                return Err(anyhow::anyhow!(
                    "kinode rng didnt respondwith "
                ));
            };
            // if they accept, create a new game
            // otherwise, should surface error to FE...
            let response = serde_json::from_slice::<RngResponse>(msg.body())?;
           
            let random = match response  {
                RngResponse::Random {result} => {
                 let random = Random { 
                        kinode: target_kinode.to_string(),
                        value: result.value,
                        timestamp: Utc::now()
                    };
                    random
                }
            };

            let body = serde_json::to_vec(&random)?;
            state.randoms.push(random);
            save_rng_state(&state);
            http::send_response(
                http::StatusCode::OK,
                Some(HashMap::from([(
                    String::from("Content-Type"),
                    String::from("application/json"),
                )])),
                body,
            );
            Ok(())
        }
        
        // Any other method will be rejected.
        _ => Ok(http::send_response(
            http::StatusCode::METHOD_NOT_ALLOWED,
            None,
            vec![],
        )),
    }
}
