mod timer;

use hyper::service::Service;
use hyper::{body::Incoming as IncomingBody, Request, Response};
use notifrust::{Notification, Timeout};
use std::collections::HashMap;
use std::io::Write;
use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::Duration;
use std::{process::Command, sync::Arc};
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::task::JoinHandle;

use http_body_util::Full;
use hyper::body::Bytes;
use hyper::server::conn::http1;
use hyper_util::rt::TokioIo;
use std::future::Future;
use std::pin::Pin;
use timer::{CurrentTime, TimerCommand, TimerConfig, TimerEvent, TimerEventHandler};
use tokio::net::TcpListener;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[tokio::main]
async fn main() -> Result<()> {
    let path = std::path::Path::new("/tmp/timer-port");

    let port = if path.exists() {
        let port = std::fs::read_to_string(path)?.parse::<u16>()?;
        if !port_is_available(port) {
            panic!("Timer already running on {port}");
        }
        port
    } else {
        let port = get_available_port().unwrap();
        let mut f = std::fs::File::create(path)?;
        write!(f, "{port}")?;
        port
    };

    let addr: SocketAddr = ([127, 0, 0, 1], port).into();

    let listener = TcpListener::bind(addr).await?;
    println!("Listening on http://{}", addr);

    let svc = Svc::new();

    loop {
        let (stream, _) = listener.accept().await?;
        let io = TokioIo::new(stream);
        let svc_clone = svc.clone();
        tokio::task::spawn(async move {
            if let Err(err) = http1::Builder::new().serve_connection(io, svc_clone).await {
                println!("Failed to serve connection: {:?}", err);
            }
        });
    }
}

fn get_available_port() -> Option<u16> {
    (3500..4000).find(|port| port_is_available(*port))
}

fn port_is_available(port: u16) -> bool {
    match std::net::TcpListener::bind(("127.0.0.1", port)) {
        Ok(_) => true,
        Err(_) => false,
    }
}

#[derive(Debug, Clone)]
struct Svc {
    timer_handle: Arc<Mutex<Option<JoinHandle<()>>>>,
    tx: Sender<TimerCommand>,
    rx: Arc<Mutex<Receiver<TimerCommand>>>,
}

impl Svc {
    pub fn new() -> Self {
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        Self {
            timer_handle: Arc::new(Mutex::new(None)),
            tx,
            rx: Arc::new(Mutex::new(rx)),
        }
    }
}

impl Service<Request<IncomingBody>> for Svc {
    type Response = Response<Full<Bytes>>;
    type Error = hyper::Error;
    type Future =
        Pin<Box<dyn Future<Output = std::result::Result<Self::Response, Self::Error>> + Send>>;

    fn call(&self, req: Request<IncomingBody>) -> Self::Future {
        fn mk_response(s: String) -> std::result::Result<Response<Full<Bytes>>, hyper::Error> {
            Ok(Response::builder().body(Full::new(Bytes::from(s))).unwrap())
        }

        let res = match req.uri().path() {
            "/start" => match req.uri().query() {
                None => mk_response("Pass minutes or seconds amount as query param".to_string()),
                Some(params) => match get_start_args(params) {
                    (Some(amount), Some(color)) => {
                        if self.timer_handle.lock().unwrap().is_some() {
                            mk_response("Timer already working".to_string())
                        } else {
                            let config = TimerConfig {
                                amount,
                                tick: Duration::from_secs(1),
                            };
                            eww_update_vairable("timer_color", color);
                            let rx = Arc::clone(&self.rx);
                            let timer_handle = Arc::clone(&self.timer_handle);
                            let handle = tokio::task::spawn(async move {
                                timer::countdown(Timer {}, config, rx).await;
                                *timer_handle.lock().unwrap() = None;
                            });
                            *self.timer_handle.lock().unwrap() = Some(handle);
                            mk_response(format!("Starting {amount:?} timer"))
                        }
                    }
                    _ => mk_response("Pass minutes amount and color as query params".to_string()),
                },
            },
            "/pause" => mk_response("Not implemented".to_string()),
            "/continue" => mk_response("Not implemented".to_string()),
            "/stop" => {
                let tx = self.tx.clone();
                tokio::task::spawn(async move {
                    let _ = tx.send(TimerCommand::Stop).await;
                });
                *self.timer_handle.lock().unwrap() = None;
                mk_response("stopping".to_string())
            }
            _ => return Box::pin(async { mk_response("oh no! not found".into()) }),
        };

        Box::pin(async { res })
    }
}

fn get_start_args(params: &str) -> (Option<Duration>, Option<String>) {
    let params = params
        .split("&")
        .filter_map(|param| param.split_once("="))
        .collect::<HashMap<_, _>>();

    println!("Got params {params:#?}");

    let amount = params
        .get("minutes")
        .map(|v| v.parse::<u64>().ok())
        .flatten()
        .map(|v| Duration::from_secs(v * 60));

    let color = params.get("color").map(ToString::to_string);

    (amount, color)
}

#[derive(Debug, Default, Clone)]
struct Timer {}

impl TimerEventHandler for Timer {
    fn handle(&self, event: timer::TimerEvent) -> timer::Result<()> {
        use TimerEvent::*;

        match event {
            Start { left } => {
                update_time(left);
            }
            Tick { left } => {
                update_time(left);
            }
            Finish => {
                clear_time();
                Notification::new()
                    .summary("Done")
                    .body("")
                    .icon("firefox")
                    .timeout(Timeout::Milliseconds(6000)) //milliseconds
                    .show()
                    .unwrap();
            }
        };

        Ok(())
    }
}

fn clear_time() {
    eww_update_vairable("timer69", "");
}

fn update_time(time: CurrentTime) {
    eww_update_vairable("timer69", format!(" {time}"));
}

fn eww_update_vairable(var: &str, value: impl AsRef<str>) {
    println!("Updating {var} to {}", value.as_ref());
    Command::new("eww")
        .arg("update")
        .arg(format!("{var}={}", value.as_ref()))
        .output()
        .unwrap();
}
