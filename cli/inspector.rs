use futures::{Future, Stream};
use futures::Sink;
use crossbeam_channel::{Sender, Receiver};
use crossbeam_channel::unbounded;
use warp::ws::WebSocket;
use warp::Filter;
use warp::ws::Message;
use serde_json::Value;

use crate::version::DENO;

static UUID: &str = "9999-622c-48ac-afec-0974f0f1d378";

lazy_static! {
    #[derive(Serialize)]
    pub static ref RESPONSE_JSON: Value = json!([{
      "description": "deno",
      "devtoolsFrontendUrl": "chrome-devtools://devtools/bundled/js_app.html?experiments=true&v8only=true&ws=127.0.0.1:9229/websocket",
      "devtoolsFrontendUrlCompat": "chrome-devtools://devtools/bundled/inspector.html?experiments=true&v8only=true&ws=127.0.0.1:9229/websocket",
      "faviconUrl": "https://www.deno-play.app/images/deno.svg",
      "id": UUID,
      "title": format!("deno[{}]", std::process::id()),
      "type": "deno",
      "url": "file://",
      "webSocketDebuggerUrl": "ws://127.0.0.1:9229/websocket"
    }]);

    #[derive(Serialize)]
    pub static ref RESPONSE_VERSION: Value = json!({
      "Browser": format!("Deno/{}", DENO),
      "Protocol-Version": "1.1",
      "webSocketDebuggerUrl": format!("ws://127.0.0.1:3012/{}", UUID)
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
    }
  }

  pub fn start(&mut self, wait: bool) {

    tokio::spawn(self.serve()); // start server

    println!("Debugger listening on ws://127.0.0.1:9229/{}", UUID);

    if wait {
      self.ready_rx.recv().unwrap();
    }

    println!("Inspector frontend connected.");
  }

  pub fn serve(&self) -> impl Future<Item = (), Error = ()> {

    let inbound_tx = self.inbound_tx.clone();
    let outbound_rx = self.outbound_rx.clone();
    let ready_tx = self.ready_tx.clone();

    let websocket = warp::path("websocket")
      .and(warp::ws2())
      .map(move |ws: warp::ws::Ws2| {

          let sender = inbound_tx.clone();
          let receiver = outbound_rx.clone();
          let ready_tx = ready_tx.clone();

          ws.on_upgrade(move |socket| on_connection(socket, sender, receiver, ready_tx))
      });

    let json = warp::path("json")
      .map(|| {

        warp::reply::json(&*RESPONSE_JSON)
      });

    let version = path!("json" / "version")
      .map(|| {

        warp::reply::json(&*RESPONSE_VERSION)
      });

    let routes = websocket
      .or(version)
      .or(json);

    warp::serve(routes).bind(([127, 0, 0, 1], 9229))
  }
}

fn on_connection(ws: WebSocket, sender: Sender<String>, receiver: Receiver<String>, ready_tx: Sender<()>) -> impl Future<Item = (), Error = ()> {

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
      let msg = receiver.recv().unwrap();
      // println!("RUST->FE: {}", msg);
      let _ = ready_tx.send(());
      user_ws_tx = user_ws_tx.send(Message::text(msg)).wait().unwrap();
    }
  });

  fut_rx
}
