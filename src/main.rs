#![allow(unused_imports)]

use anyhow::{anyhow, Error, Result};
use futures::TryFutureExt;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server, StatusCode, Uri};
use std::net::SocketAddr;
use std::str::FromStr;
use tokio::process::Command;
//use std::convert::Infallible;
//use {std::time::Duration, tokio::time::sleep};

const YTDL: &str = "yt-dlp";
const USAGE: &str = "Usage: GET /<URL>/[cover.*]";

type Client = hyper::Client<hyper_tls::HttpsConnector<hyper::client::HttpConnector>>;

#[tokio::main]
async fn main() {
    let client = hyper::Client::builder().build::<_, Body>(hyper_tls::HttpsConnector::new());

    // A `MakeService` that produces a `Service` to handle each connection.
    let make_service = make_service_fn(move |_socket| {
        let client = client.clone();

        // Create a `Service` for responding to the request.
        let service = service_fn(move |request| {
            let client = client.clone();
            async {
                handle_request(request, client).await.or_else(|e| {
                    eprintln!("{e}");
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
        eprintln!("server error: {e}");
    }
}

async fn handle_request(mut request: Request<Body>, client: Client) -> Result<Response<Body>> {
    let (input, is_asking_cover) = extract_input(request.uri().path_and_query().unwrap().as_str())?;
    dbg!(&input);

    if is_asking_cover {
        dbg!("Asked for cover. NOT FOUND.");
        return Ok::<_, Error>(
            Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::empty())?,
        );
    }

    let stream_url = ask_stream_url(input).await?;
    dbg!(stream_url.get(0..65));

    *request.uri_mut() = Uri::from_str(&stream_url)?;
    request.headers_mut().remove("host");
    dbg!(&request);
    let response = client.request(request).await?;
    dbg!(&response);

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

async fn ask_stream_url(input: String) -> Result<String> {
    let child = Command::new(YTDL)
        .args(["-f", "bestaudio", "-g", &input])
        .stdout(std::process::Stdio::piped())
        .spawn()?;

    let output = child.wait_with_output().await?;

    Ok(match output.status.success() {
        true => {
            let stream_url = std::str::from_utf8(&output.stdout)?.trim_end().to_string();
            if stream_url.is_empty() {
                Err(anyhow!("received empty stream_url from {YTDL}."))?
            }
            stream_url
        }
        false => Err(anyhow!("child process failed."))?,
    })
}
