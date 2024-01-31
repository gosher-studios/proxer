use std::fs::{File, self};
use std::net::SocketAddr;
use std::collections::HashMap;
use std::sync::Arc;
use std::io::BufReader;
use tokio::task;
use tokio::net::{TcpListener, TcpStream};
use hyper::header;
use hyper::header::HeaderValue;
use hyper::server::conn::http1 as server;
use hyper::client::conn::http1 as client;
use hyper::service::service_fn;
use tls_listener::TlsListener;
use tls_listener::rustls::TlsAcceptor;
use tls_listener::rustls::rustls;
use serde::Deserialize;

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Deserialize)]
struct Config {
  listen: SocketAddr,
  cert_chain: String,
  private_key: String,
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
  let acceptor = TlsAcceptor::from(Arc::new(
    rustls::ServerConfig::builder()
      .with_safe_defaults()
      .with_no_client_auth()
      .with_single_cert(
        rustls_pemfile::certs(&mut BufReader::new(File::open(config.cert_chain)?))?
          .iter()
          .map(|c| rustls::Certificate(c.clone()))
          .collect(),
        rustls::PrivateKey(
          rustls_pemfile::pkcs8_private_keys(&mut BufReader::new(File::open(config.private_key)?))?
            [0]
            .clone(),
        ),
      )?,
  ));
  let mut listener = TlsListener::new(acceptor, TcpListener::bind(config.listen).await?);

  println!("listening on {}", config.listen);
  for (host, addr) in &config.services {
    println!("{} -> {}", host, addr);
  }
  loop {
    if let Some(Ok(stream)) = listener.accept().await {
      task::spawn({
        let services = config.services.clone();
        async move {
          let ip = &stream.get_ref().0.peer_addr().unwrap();
          server::Builder::new()
            .serve_connection(
              stream,
              service_fn(|mut req| async {
                let local_stream = TcpStream::connect(
                  services
                    .get(req.headers().get(header::HOST).unwrap().to_str().unwrap())
                    .unwrap(),
                )
                .await
                .unwrap();
                let (mut sender, conn) = client::handshake(local_stream).await?;
                task::spawn(async move { conn.await });
                dbg!(&ip)
                req.headers_mut().insert("X-Forwarded-For", HeaderValue::from_str(&ip.to_string()).unwrap());
                sender.send_request(req).await
              }),
            )
            .await
            .unwrap()
        }
      });
    }
  }
}
