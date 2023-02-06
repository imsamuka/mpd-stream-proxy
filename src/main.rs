use anyhow::{anyhow, Error, Result};
use hyper::{body::HttpBody, Body, Request, Response, Server, StatusCode, Uri};

use log::{debug, error, info, trace, warn};
use moka::future::Cache;
use serde_json::Value;
use std::{cmp::Ordering, str::FromStr, sync::Arc, time::Duration};

const YTDL: &str = "yt-dlp";
const USAGE: &str = "Usage: GET /<URL>/[cover.*]";

#[derive(Clone)]
struct Context {
    client: hyper::Client<hyper_tls::HttpsConnector<hyper::client::HttpConnector>>,
    ytdl_cache: Cache<String, Arc<Value>>,
}

#[tokio::main]
async fn main() {
    use hyper::service::{make_service_fn, service_fn};

    simple_logger::init_with_env().unwrap();

    let cx = Context {
        client: hyper::Client::builder().build::<_, Body>(hyper_tls::HttpsConnector::new()),
        ytdl_cache: Cache::builder()
            .initial_capacity(10)
            .time_to_live(Duration::from_secs(600))
            .build(),
    };

    // A `MakeService` that produces a `Service` to handle each connection.
    let make_service = make_service_fn(move |_socket| {
        let cx = cx.clone();

        let service = service_fn(move |request| {
            let cx = cx.clone();
            async {
                handle_request(request, cx).await.or_else(|e| {
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

    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], 4000));
    let server = Server::bind(&addr).serve(make_service);

    if let Err(e) = server.await {
        error!("Server error: {e}");
    }
}

async fn handle_request(mut request: Request<Body>, cx: Context) -> Result<Response<Body>> {
    let Context {
        client,
        ytdl_cache: cache,
    } = cx;

    let (input, cover_ext, is_asking_cover) =
        extract_input(request.uri().path_and_query().unwrap().as_str())?;
    info!("input: {input}");

    let info = if let Some(info) = cache.get(&input) {
        info
    } else {
        info!("updating cache");

        for info in ask_stream_infos(&input).await? {
            let key = key_from_info(&info)?.to_string();
            cache.insert(key, Arc::new(info)).await;
        }
        cache
            .get(&input)
            .expect(r#""input" to be equal to one "original_url""#)
    };

    let proxied_url = if is_asking_cover {
        info!("asking for cover.{}", &cover_ext);
        cover_url_from_info(&info, &cover_ext)?
    } else {
        stream_url_from_info(&info)?
    };

    debug!("proxied_url: {}", proxied_url);

    *request.uri_mut() = Uri::from_str(proxied_url)?;
    request.headers_mut().remove("host");

    trace!("request: {request:#?}");
    let response = client.request(request).await?;
    debug!("response: {response:#?}");

    Ok(response)
}

fn extract_input(path_and_query: &str) -> Result<(String, String, bool)> {
    let (input, last) = path_and_query.rsplit_once('/').unwrap_or_default();

    let is_asking_cover = last.starts_with("cover.");
    if !last.is_empty() && !is_asking_cover {
        Err(anyhow!("No '/' or '/cover.*' after the URL."))?
    }

    let input = input.trim_start_matches('/');
    if input.is_empty() {
        Err(anyhow!("Empty URL. {USAGE}"))?
    }

    let cover_ext = if is_asking_cover {
        let cover_ext = last.split_once('.').expect("cover to have a '.'").1;
        if cover_ext.is_empty() {
            Err(anyhow!("cover asked has no extension"))?
        }
        cover_ext.to_string()
    } else {
        String::new()
    };

    Ok((input.to_string(), cover_ext, is_asking_cover))
}

fn key_from_info(info: &Value) -> Result<&str> {
    info.get("original_url")
        .ok_or(anyhow!("no \"original_url\" present in info"))?
        .as_str()
        .ok_or(anyhow!("\"original_url\" is not a string"))
}

fn stream_url_from_info(info: &Value) -> Result<&str> {
    info.get("url")
        .ok_or(anyhow!("no \"url\" present in info"))?
        .as_str()
        .ok_or(anyhow!("\"url\" is not a string"))
}

fn cover_url_from_info<'a>(info: &'a Value, cover_ext: &str) -> Result<&'a str> {
    struct Thumb<'a> {
        url: &'a str,
        preference: i64,
    }

    info.get("thumbnails")
        .ok_or(anyhow!("no \"thumbnails\" present in info"))?
        .as_array()
        .ok_or(anyhow!("\"thumbnails\" is not a array"))?
        .iter()
        .filter_map(|t| {
            Some(Thumb {
                url: t.get("url")?.as_str()?,
                preference: t.get("preference")?.as_i64()?,
            })
        })
        .filter(|t| t.url.ends_with(cover_ext))
        .reduce(|prev, next| match prev.preference.cmp(&next.preference) {
            Ordering::Less | Ordering::Equal => next,
            Ordering::Greater => prev,
        })
        .map(|t| t.url)
        .ok_or(anyhow!(
            "no valid thumbnail found for extension '{cover_ext}'"
        ))
}

async fn ask_stream_infos(input: &str) -> Result<Vec<Value>> {
    let child = tokio::process::Command::new(YTDL)
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
