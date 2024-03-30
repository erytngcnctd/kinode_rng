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


#[derive(Debug, Serialize, Deserialize)]
enum RngRequest {
    NewRandom { context: Option<String> , range: (u64,u64)},
    // History,
}

#[derive(Debug, Serialize, Deserialize)]
enum RngResponse {
    Random { result: Random },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Random {
    rng_source: String,
    msg_source: String,
    range: (u64,  u64),
    value: u64,
    context: Option<String>,
    timestamp: chrono::DateTime<chrono::Utc>
}



#[derive(Debug, Serialize, Deserialize)]
struct RngState {
    pub randoms: Vec<Random>, 
    pub clients: HashSet<u32>,       
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredRngState {
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
                    message_type: http::WsMessageType::Text,
                },
            )?)
            .blob(LazyLoadBlob {
                mime: Some("application/json".to_string()),
                bytes: serde_json::json!({
                    "kind": "NewRandom",
                    "data": random,
                })
                .to_string()
                .as_bytes()
                .to_vec(),
            })
            .send()?;
    }
    Ok(())
}

wit_bindgen::generate!({
    path: "wit",
    world: "process",
    exports: {
        world: Component,
    },
});

call_init!(initialize);

fn initialize(our: Address) {
  
    println!("started");
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

    http::serve_ui(&our, "ui", true, false, vec!["/"]).unwrap();

    http::bind_http_path("/randoms", true, false).unwrap();

    http::bind_ws_path("/", true, false).unwrap();

    let mut state: RngState = load_rng_state();
    main_loop(&our, &mut state);
}

fn main_loop(our: &Address, state: &mut RngState) {
    loop {
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

fn handle_request(our: &Address, message: &Message, state: &mut RngState) -> anyhow::Result<()> {

    if !message.is_request() {
        return Ok(());
    }
   
    if message.source().node != our.node {
       
        let Ok(rng_request) = serde_json::from_slice::<RngRequest>(message.body()) else {
            return Err(anyhow::anyhow!("invalid rng request"));
        };
        println!("rng_request");
        handle_rng_request(our, &message.source().node, state, &rng_request)
   
    } else if message.source().node == our.node
        && message.source().process == "terminal:terminal:sys"
    {
        let Ok(rng_request) = serde_json::from_slice::<RngRequest>(message.body()) else {
            return Err(anyhow::anyhow!("invalid rng request"));
        };
        handle_local_request(our, state, &rng_request)
    } else if message.source().node == our.node
        && message.source().process == "http_server:distro:sys"
    {
    
        match serde_json::from_slice::<http::HttpServerRequest>(message.body())? {
            http::HttpServerRequest::Http(ref incoming) => {

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

            http::HttpServerRequest::WebSocketOpen { channel_id, .. } => {
                state.clients.insert(channel_id);
                Ok(())
            }
            http::HttpServerRequest::WebSocketClose(channel_id) => {
                state.clients.remove(&channel_id);
                Ok(())
            }
            http::HttpServerRequest::WebSocketPush { .. } => {
                Ok(())
            }
        }
    } else {
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

    let kinode = source_node;

    match action {
        RngRequest::NewRandom  { context, range} => {
            println!{"request"}
            if range.0 > range.1 {
                return Err(anyhow::anyhow!("invalid number range"));
            }
            let mut rng = ChaCha20Rng::from_entropy();
            let random = Random {
                rng_source: our.node.to_string(),
                msg_source: kinode.clone(), 
                range: range.clone(), 
                value: rng.gen_range(range.0..=range.1),
                context: context.clone(),
                timestamp: Utc::now()
            };
            println!("value: {}",random.value);
            state.randoms.push(random.clone());
            save_rng_state(&state);
          
            send_ws_update(&our, &random, &state.clients)?;
          
            Response::new()
                .body(serde_json::to_vec(&RngResponse::Random{result: random.clone()})?)
                .send()
        }
       
    }
}

fn handle_local_request(
    our: &Address,
    state: &mut RngState,
    action: &RngRequest,
) -> anyhow::Result<()> {
    match action {
        RngRequest::NewRandom { context, range } => {
            println!("here local");
            if range.0 > range.1 {
                return Err(anyhow::anyhow!("invalid number range"));
            }
            //check value max > min??
            let mut rng = ChaCha20Rng::from_entropy();
            let random = Random {
                rng_source: our.node.clone().to_string(),
                msg_source: our.node.clone().to_string(),
                range: range.clone(), 
                value: rng.gen_range(range.0..range.1+1),
                context: context.clone(),
                timestamp: Utc::now()
            };
            state.randoms.push(random.clone());
            println!("value: {}",random.value);
            save_rng_state(&state);
            let body = serde_json::to_vec(&random)?;
            http::send_response(
                http::StatusCode::OK,
                Some(HashMap::from([(
                    String::from("Content-Type"),
                    String::from("application/json"),
                )])),
                body,
            );
            send_ws_update(&our, &random, &state.clients)?;
            Ok(())
        }
    }
}

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
       
        "GET" => Ok(http::send_response(
            http::StatusCode::OK,
            Some(HashMap::from([(
                String::from("Content-Type"),
                String::from("application/json"),
            )])),
            serde_json::to_vec(&state.randoms)?,
        )),
     
        "POST" => {
            let Some(blob) = get_blob() else {
                return Ok(http::send_response(
                    http::StatusCode::BAD_REQUEST,
                    None,
                    vec![],
                ));
            };
            let blob_json = serde_json::from_slice::<serde_json::Value>(&blob.bytes)?;
            let (Some(target_kinode), Some(min), Some(max)) = (blob_json["target"].as_str(),
                blob_json["range"]["min"].as_u64(),blob_json["range"]["max"].as_u64() )  else {
                return Ok(http::send_response(
                    http::StatusCode::BAD_REQUEST,
                    None,
                    vec![],
                ));
            };
            // if min > max  {
            //     return Ok(http::send_response(
            //     http::StatusCode::BAD_REQUEST,
            //     None,
            //     vec![],
            //     ));
            // };
            let context = blob_json["context"].as_str();
           
               //change this
                let action = &RngRequest::NewRandom { range: (min,max), context: context.map(str::to_string) };
                
                if target_kinode == our.node { 
                   handle_local_request(our, state, &action)} else {
                    println!("here http");
                let Ok(msg) = Request::new()
                    .target((target_kinode, our.process.clone()))
                    .body(serde_json::to_vec(action)?)
                    .send_and_await_response(5)?
                    else {
                        return Err(anyhow::anyhow!(
                            "no response. . ."
                        ))
                    };
                
                let response = serde_json::from_slice::<RngResponse>(msg.body())?;
            
                let random = match response  {
                    RngResponse::Random {result} => {
                    let random = Random { 
                            rng_source: target_kinode.to_string(),
                            msg_source: our.node.to_string(),
                            range: (min,max),
                            value: result.value,
                            context: context.map(str::to_string),
                            timestamp: Utc::now()
                        };
                        random
                    }
                };
                state.randoms.push(random.clone());
                save_rng_state(&state);



                let body = serde_json::to_vec(&random)?;
              
                send_ws_update(&our, &random, &state.clients)?;
                // Send a WebSocket message to the http server in order to update the UI
               

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
        }

        _ => Ok(http::send_response(
            http::StatusCode::METHOD_NOT_ALLOWED,
            None,
            vec![],
        )),
    }
}
