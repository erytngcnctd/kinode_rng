extern crate rand;
extern crate rand_chacha;
extern crate chrono;

use std::collections::HashMap;
use std::str::FromStr;

use rand::prelude::*;
use rand_chacha::ChaCha20Rng;
use chrono::Utc;

use kinode_process_lib::{
    await_message, call_init, get_blob,
    http::{
        bind_http_path, bind_ws_path, send_response, send_ws_push, serve_ui, HttpServerRequest,
        IncomingHttpRequest, StatusCode, WsMessageType,
    },
    println, Address, LazyLoadBlob, Message, ProcessId, Request, Response,
};
use serde::{Deserialize, Serialize};

wit_bindgen::generate!({
    path: "wit",
    world: "process",
    exports: {
        world: Component,
    },
});

#[derive(Debug, Serialize, Deserialize)]
enum RngRequest {
    Send { target: String, random: usize },
    History,
}

#[derive(Debug, Serialize, Deserialize)]
enum RngResponse {
    Ack,
    History { randoms: RandomsArchive },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct RngMessage {
    kinode: String,
    random: String,
    timestamp: chrono::DateTime<chrono::Utc>
}


type RandomsArchive = Vec<RngMessage>;

fn handle_http_server_request(
    our: &Address,
    randoms_archive: &mut RandomsArchive,
    our_channel_id: &mut u32,
    source: &Address,
    body: &[u8],
) -> anyhow::Result<()> {
    let Ok(server_request) = serde_json::from_slice::<HttpServerRequest>(body) else {
        // Fail silently if we can't parse the request
        return Ok(());
    };

    match server_request {
        HttpServerRequest::WebSocketOpen { channel_id, .. } => {
            // Set our channel_id to the newly opened channel
            // Note: this code could be improved to support multiple channels
            *our_channel_id = channel_id;
        }
        HttpServerRequest::WebSocketPush { .. } => {
            let Some(blob) = get_blob() else {
                return Ok(());
            };

            handle_chat_request(
                our,
                randoms_archive,
                our_channel_id,
                source,
                &blob.bytes,
                false,
            )?;
        }
        HttpServerRequest::WebSocketClose(_channel_id) => {}
        HttpServerRequest::Http(request) => {
            match request.method()?.as_str() {
                // Get all messages
                "GET" => {
                    let mut headers = HashMap::new();
                    headers.insert("Content-Type".to_string(), "application/json".to_string());

                    send_response(
                        StatusCode::OK,
                        Some(headers),
                        serde_json::to_vec(&RngResponse::History {
                            randoms: randoms_archive.clone(),
                        })
                        .unwrap(),
                    );
                }
                // Send a message
                "POST" => {
                    let Some(blob) = get_blob() else {
                        return Ok(());
                    };
                    handle_random_request(
                        our,
                        randoms_archive,
                        our_channel_id,
                        source,
                        &blob.bytes,
                        true,
                    )?;

                    // Send an http response via the http server
                    send_response(StatusCode::CREATED, None, vec![]);
                }
                _ => {
                    // Method not allowed
                    send_response(StatusCode::METHOD_NOT_ALLOWED, None, vec![]);
                }
            }
        }
    };

    Ok(())
}

fn handle_chat_request(
    our: &Address,
    randoms_archive: &mut RandomsArchive,
    channel_id: &mut u32,
    source: &Address,
    body: &[u8],
    is_http: bool,
) -> anyhow::Result<()> {
    let Ok(rng_request) = serde_json::from_slice::<RngRequest>(body) else {
        // Fail silently if we can't parse the request
        return Ok(());
    };

    match rng_request {
        RngRequest::NewRandom {
            ref target,
            // ref random,
        } => {
            // counterparty will be the other node in the chat with us
            let (counterparty, kinode) = if target == &our.node {
                (&source.node, source.node.clone())
            } else {
                (target, our.node.clone())
            };

            // If the target is not us, send a request to the target

            if target != &our.node {
                println!("new rng request from {}: {}", source.node, random);

                match Request::new()
                    .target(Address {
                        node: target.clone(),
                        process: ProcessId::from_str("kinode_rng:kinode_rng:magiccity.os")?,
                    })
                    .body(body)
                    .send_and_await_response(5)
                {
                    Ok(_) => {}
                    Err(e) => {
                        println!("testing: send request error: {:?}", e);
                        return Ok(());
                    }
                };
            }

            // Retreive the message archive for the counterparty, or create a new one if it doesn't exist


            let new_random= RngMessage {
                kinode: kinode.clone(),
                random: random.clone(),
                timestamp: Utc::now()
            };

            // If this is an HTTP request, handle the response in the calling function
            if is_http {
                // Add the new message to the archive
                randoms.push(new_random);
                return Ok(());
            }

            // If this is not an HTTP request, send a response to the other node
            Response::new()
                .body(serde_json::to_vec(&ChatResponse::Ack).unwrap())
                .send()
                .unwrap();

            // Add the new message to the archive
            messages.push(new_message);

            // Generate a blob for the new message
            let blob = LazyLoadBlob {
                mime: Some("application/json".to_string()),
                bytes: serde_json::json!({
                    "NewMessage": NewMessage {
                        chat: counterparty.clone(),
                        author,
                        content: message.clone(),
                    }
                })
                .to_string()
                .as_bytes()
                .to_vec(),
            };

            // Send a WebSocket message to the http server in order to update the UI
            send_ws_push(
                channel_id.clone(),
                WsMessageType::Text,
                blob,
            );
        }
        ChatRequest::History => {
            // If this is an HTTP request, send a response to the http server

            Response::new()
                .body(
                    serde_json::to_vec(&ChatResponse::History {
                        messages: randoms_archive.clone(),
                    })
                    .unwrap(),
                )
                .send()
                .unwrap();
        }
    };

    Ok(())
}

fn handle_message(
    our: &Address,
    randoms_archive: &mut RandomsArchive,
    channel_id: &mut u32,
) -> anyhow::Result<()> {
    let message = await_message().unwrap();

    match message {
        Message::Response { .. } => {
            println!("kinode_rng: got response - {:?}", message);
            return Ok(());
        }
        Message::Request {
            ref source,
            ref body,
            ..
        } => {
            // Requests that come from other nodes running this app
            handle_chat_request(our, randoms_archive, channel_id, source, body, false)?;
            // Requests that come from our http server
            handle_http_server_request(our, randoms_archive, channel_id, source, body)?;
        }
    }

    Ok(())
}

call_init!(init);

fn init(our: Address) {
    println!("kinode_rng: begin");

    let mut randoms_archive: RandomsArchive = HashMap::new();
    let mut channel_id = 0;

    // Bind UI files to routes; index.html is bound to "/"
    serve_ui(&our, "ui", true, true, vec!["/"]).unwrap();

    // Bind HTTP path /messages
    bind_http_path("/messages", true, false).unwrap();

    // Bind WebSocket path
    bind_ws_path("/", true, false).unwrap();

    loop {
        match handle_message(&our, &mut randoms_archive, &mut channel_id) {
            Ok(()) => {}
            Err(e) => {
                println!("kinode_rng: error: {:?}", e);
            }
        };
    }
}
