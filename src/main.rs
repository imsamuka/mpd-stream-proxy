#![allow(unused_imports)]

use anyhow::{anyhow, Error, Result};
use futures::TryFutureExt;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server, StatusCode, Uri};
use log::{debug, error, info, trace, warn};
use serde_json::Value;
use std::net::SocketAddr;
use std::str::FromStr;
use tokio::process::Command;
//use {std::time::Duration, tokio::time::sleep};

const YTDL: &str = "yt-dlp";
const USAGE: &str = "Usage: GET /<URL>/[cover.*]";

type Client = hyper::Client<hyper_tls::HttpsConnector<hyper::client::HttpConnector>>;

#[tokio::main]
async fn main() {
    simple_logger::init_with_env().unwrap();

    let client = hyper::Client::builder().build::<_, Body>(hyper_tls::HttpsConnector::new());

    // A `MakeService` that produces a `Service` to handle each connection.
    let make_service = make_service_fn(move |_socket| {
        let client = client.clone();

        // Create a `Service` for responding to the request.
        let service = service_fn(move |request| {
            let client = client.clone();
            async {
                handle_request(request, client).await.or_else(|e| {
                    error!("Request error: {e}");
                    Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(Body::empty())
                })
            }
        });

        // Return the service to hyper.
        async move { Ok::<_, Error>(service) }
    });

    let addr = SocketAddr::from(([127, 0, 0, 1], 4000));
    let server = Server::bind(&addr).serve(make_service);

    if let Err(e) = server.await {
        error!("Server error: {e}");
    }
}

async fn handle_request(mut request: Request<Body>, client: Client) -> Result<Response<Body>> {
    let (input, is_asking_cover) = extract_input(request.uri().path_and_query().unwrap().as_str())?;
    info!("received input: {input}");

    if is_asking_cover {
        Err(anyhow!("Asked for cover."))?
    }

    let input_info = ask_stream_infos(&input).await?;
    let stream_url = stream_url_from_info(&input_info[0])?;
    debug!("stream_url: {:?}", stream_url.get(0..65));

    *request.uri_mut() = Uri::from_str(stream_url)?;
    request.headers_mut().remove("host");
    debug!("request: {request:#?}");
    let response = client.request(request).await?;
    debug!("response: {response:#?}");

    Ok(response)
}

fn extract_input(path_and_query: &str) -> Result<(String, bool)> {
    let (input, last) = path_and_query.rsplit_once('/').unwrap_or_default();
    let is_asking_cover = last.starts_with("cover");
    if !last.is_empty() && !is_asking_cover {
        Err(anyhow!("No '/' or '/cover.*' after the URL."))?
    }
    let input = input.trim_start_matches('/');
    if input.is_empty() {
        Err(anyhow!("Empty URL. {USAGE}"))?
    }
    Ok((input.to_string(), is_asking_cover))
}

fn stream_url_from_info(info: &Value) -> Result<&str> {
    info.get("url")
        .ok_or(anyhow!("no \"url\" present in info"))?
        .as_str()
        .ok_or(anyhow!("\"url\" is not a string"))
}

async fn ask_stream_infos(input: &str) -> Result<Vec<Value>> {
    let child = Command::new(YTDL)
        .args(["-f", "bestaudio", "-j", input])
        .stdout(std::process::Stdio::piped())
        .spawn()?;

    let output = child.wait_with_output().await?;

    match output.status.success() {
        true => {
            let mut infos = Vec::<Value>::with_capacity(20); // bigger than most albums

            for raw_json in std::str::from_utf8(&output.stdout)?.lines() {
                match serde_json::from_str::<Value>(raw_json) {
                    Ok(info) => infos.push(info),
                    Err(e) => warn!("couldn't parse JSON: {e}"),
                }
            }

            if infos.is_empty() {
                Err(anyhow!("received no info from {YTDL}."))?
            }

            Ok(infos)
        }
        false => Err(anyhow!("child process failed to gather info."))?,
    }
}

#[allow(unused)]
async fn ask_stream_url(input: &str) -> Result<String> {
    let child = Command::new(YTDL)
        .args(["-f", "bestaudio", "-g", input])
        .stdout(std::process::Stdio::piped())
        .spawn()?;

    let output = child.wait_with_output().await?;

    match output.status.success() {
        true => {
            let stream_url = std::str::from_utf8(&output.stdout)?.trim_end().to_string();
            if stream_url.is_empty() {
                Err(anyhow!("received empty stream_url from {YTDL}."))?
            }
            Ok(stream_url)
        }
        false => Err(anyhow!("child process failed."))?,
    }
}
