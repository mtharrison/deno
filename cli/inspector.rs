use futures::{Future, Stream};
use futures::Sink;
use futures::sync::oneshot::{SpawnHandle, spawn};
use crossbeam_channel::{Sender, Receiver};
use crossbeam_channel::unbounded;
use warp::ws::WebSocket;
use warp::Filter;
use warp::ws::Message;
use serde_json::Value;
use std::sync::{Arc, Mutex};

use crate::version::DENO;

static UUID: &str = "9999-622c-48ac-afec-0974f0f1d378";
static HOST: &str = "127.0.0.1";
static PORT: &str = "9888";

lazy_static! {
    #[derive(Serialize)]
    pub static ref RESPONSE_JSON: Value = json!([{
      "description": "deno",
      "devtoolsFrontendUrl": format!("chrome-devtools://devtools/bundled/js_app.html?experiments=true&v8only=true&ws={}:{}/websocket", HOST, PORT),
      "devtoolsFrontendUrlCompat": format!("chrome-devtools://devtools/bundled/inspector.html?experiments=true&v8only=true&ws={}:{}/websocket", HOST, PORT),
      "faviconUrl": "https://www.deno-play.app/images/deno.svg",
      "id": UUID,
      "title": format!("deno[{}]", std::process::id()),
      "type": "deno",
      "url": "file://",
      "webSocketDebuggerUrl": format!("ws://{}:{}/websocket", HOST, PORT)
    }]);

    #[derive(Serialize)]
    pub static ref RESPONSE_VERSION: Value = json!({
      "Browser": format!("Deno/{}", DENO),
      "Protocol-Version": "1.1",
      "webSocketDebuggerUrl": format!("ws://{}:{}/{}", HOST, PORT, UUID)
    });
}

pub struct Inspector {
  // sharable handle to channels passed to isolate
  pub handle: deno::InspectorHandle,
  // sending/receving messages from isolate
  inbound_tx: Sender<String>,
  outbound_rx: Receiver<String>,
  // signals readiness of inspector
  ready_tx: Sender<()>,
  ready_rx: Receiver<()>,
  server_spawn_handle: Option<SpawnHandle<(), ()>>,
  connected: Arc<Mutex<bool>>,
}

impl Inspector {
  pub fn new() -> Self {

    let (inbound_tx, inbound_rx) = unbounded::<String>();
    let (outbound_tx, outbound_rx) = unbounded::<String>();
    let (ready_tx, ready_rx) = unbounded::<()>();

    Inspector {
      handle: deno::InspectorHandle::new(outbound_tx, inbound_rx),
      inbound_tx,
      outbound_rx,
      ready_rx,
      ready_tx,
      server_spawn_handle: None,
      connected: Arc::new(Mutex::new(false)),
    }
  }

  pub fn serve(&self) -> impl Future<Item = (), Error = ()> {

    let inbound_tx = self.inbound_tx.clone();
    let outbound_rx = self.outbound_rx.clone();
    let ready_tx = self.ready_tx.clone();
    let connected = self.connected.clone();

    let websocket = warp::path("websocket")
      .and(warp::ws2())
      .map(move |ws: warp::ws::Ws2| {

          let sender = inbound_tx.clone();
          let receiver = outbound_rx.clone();
          let ready_tx = ready_tx.clone();
          let connected = connected.clone();

          ws.on_upgrade(move |socket| on_connection(socket, sender, receiver, ready_tx, connected))
      });

    let json = warp::path("json").map(|| warp::reply::json(&*RESPONSE_JSON));

    let version = path!("json" / "version").map(|| warp::reply::json(&*RESPONSE_VERSION));

    let routes = websocket.or(version).or(json);

    let endpoint = format!("{}:{}", HOST, PORT);
    let addr = endpoint.parse::<std::net::SocketAddrV4>().unwrap();

    warp::serve(routes).bind(addr)
  }

  pub fn start(&mut self, wait: bool) {

    self.server_spawn_handle = Some(spawn(self.serve(), &tokio_executor::DefaultExecutor::current()));

    println!("Debugger listening on ws://{}:{}/{}", HOST, PORT, UUID);

    if wait {
      self.ready_rx.recv().expect("Error waiting for inspector server to start.");
      println!("Inspector frontend connected.");
    }
  }

  pub fn stop(&mut self) {}
}

fn on_connection(
  ws: WebSocket,
  sender: Sender<String>,
  receiver: Receiver<String>,
  ready_tx: Sender<()>,
  connected: Arc<Mutex<bool>>,
) -> impl Future<Item = (), Error = ()> {

  let (mut user_ws_tx, user_ws_rx) = ws.split();

  let fut_rx = user_ws_rx
      .for_each(move |msg| {
          let msg_str = msg.to_str().unwrap();
          // println!("FE->RUST: {}", msg_str);
          sender.send(msg_str.to_owned()).unwrap_or_else(|err| println!("Err: {}", err));

          Ok(())
      })
      .map_err(|_|{});

  std::thread::spawn(move || {
    loop {
      let received = receiver.recv();
      if let Ok(msg) = received {
        let _ = ready_tx.send(());
        *connected.lock().unwrap() = true;
        user_ws_tx = user_ws_tx.send(Message::text(msg)).wait().unwrap();
      }
    }
  });

  fut_rx
}

impl Drop for Inspector {
  fn drop(&mut self) {
    if *self.connected.lock().unwrap() == true {
      println!("Waiting for debugger to disconnect...");
    }
  }
}
