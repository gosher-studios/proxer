use std::fs;
use std::net::SocketAddr;
use std::collections::HashMap;
use tokio::task;
use tokio::net::{TcpListener, TcpStream};
use hyper::header;
use hyper::server::conn::http1 as server;
use hyper::client::conn::http1 as client;
use hyper::service::service_fn;
use serde::Deserialize;

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Deserialize)]
struct Config {
  listen: SocketAddr,
  services: HashMap<String, SocketAddr>,
}

impl Config {
  fn load() -> Result<Self> {
    Ok(toml::from_str(&fs::read_to_string("proxy.toml")?)?)
  }
}

#[tokio::main]
async fn main() -> Result {
  let config = Config::load()?;
  let listener = TcpListener::bind(config.listen).await?;
  println!("listening on {}", config.listen);
  for (host, addr) in &config.services {
    println!("{} -> {}", host, addr);
  }
  loop {
    let (stream, _) = listener.accept().await?;
    task::spawn({
      let services = config.services.clone();
      async move {
        server::Builder::new()
          .serve_connection(
            stream,
            service_fn(|req| async {
              let stream = TcpStream::connect(
                services
                  .get(req.headers().get(header::HOST).unwrap().to_str().unwrap())
                  .unwrap(),
              )
              .await
              .unwrap();
              let (mut sender, conn) = client::handshake(stream).await?;
              task::spawn(async move { conn.await });
              sender.send_request(req).await
            }),
          )
          .await
          .unwrap()
      }
    });
  }
}
